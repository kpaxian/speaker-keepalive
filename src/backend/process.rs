//! Bounded synchronous child I/O. No blocking writes and no reader threads.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

const STDERR_LIMIT: usize = 8 * 1024;
const STDOUT_LIMIT: usize = 32 * 1024;
const IO_CHUNK: usize = 16 * 1024;

#[derive(Clone, Default)]
pub(super) struct Cancellation(Arc<CancelState>);

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    idle: Mutex<()>,
    wake: Condvar,
}

impl Cancellation {
    pub(super) fn install() -> io::Result<Self> {
        let cancellation = Self::default();
        let handler = cancellation.clone();
        ctrlc::set_handler(move || handler.cancel())
            .map_err(|error| io::Error::other(format!("cannot install Ctrl+C handler: {error}")))?;
        Ok(cancellation)
    }

    pub(super) fn cancel(&self) {
        let _guard = self.0.idle.lock().unwrap();
        self.0.cancelled.store(true, Ordering::Relaxed);
        self.0.wake.notify_all();
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }

    /// The mutex prevents a notification being lost between check and sleep.
    pub(super) fn wait_until(&self, deadline: Instant) -> io::Result<()> {
        let mut guard = self.0.idle.lock().unwrap();
        loop {
            if self.is_cancelled() {
                return Err(cancelled());
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            guard = self.0.wake.wait_timeout(guard, deadline - now).unwrap().0;
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub timeout: Option<Duration>,
    pub no_progress: Duration,
    pub poll_interval: Duration,
    pub terminate_grace: Duration,
}

impl Limits {
    pub(super) fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            no_progress: Duration::from_secs(10),
            poll_interval: Duration::from_millis(100),
            terminate_grace: Duration::from_millis(500),
        }
    }
}

#[derive(Default, Debug)]
pub(super) struct Capture {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

impl Capture {
    pub(super) fn text(&self) -> String {
        let suffix = if self.truncated { " [truncated]" } else { "" };
        format!("{}{suffix}", String::from_utf8_lossy(&self.bytes).trim())
    }

    fn drain<R: Read>(&mut self, pipe: &mut Option<R>, limit: usize) -> io::Result<()> {
        let Some(reader) = pipe.as_mut() else {
            return Ok(());
        };
        let mut buffer = [0; 4096];
        // A permanently noisy child must not starve deadline/cancellation checks.
        for _ in 0..16 {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    *pipe = None;
                    return Ok(());
                }
                Ok(count) => {
                    let keep = count.min(limit - self.bytes.len());
                    self.bytes.extend_from_slice(&buffer[..keep]);
                    self.truncated |= keep != count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[derive(Default, Debug)]
pub(super) struct Captured {
    pub stdout: Capture,
    pub stderr: Capture,
}

pub(super) fn execute(
    command: &mut Command,
    input: Option<&[u8]>,
    capture_stdout: bool,
    cancellation: &Cancellation,
    limits: Limits,
) -> io::Result<Captured> {
    execute_input(
        command,
        input.map(Input::Finite),
        capture_stdout,
        cancellation,
        limits,
    )
}

pub(super) enum Input<'a> {
    Finite(&'a [u8]),
    Repeat {
        buffer: &'a [u8],
        progress: &'a mut dyn FnMut(),
    },
}

pub(super) fn execute_input(
    command: &mut Command,
    mut input: Option<Input<'_>>,
    capture_stdout: bool,
    cancellation: &Cancellation,
    limits: Limits,
) -> io::Result<Captured> {
    if matches!(&input, Some(Input::Repeat { buffer, .. }) if buffer.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repeating input must not be empty",
        ));
    }
    if cancellation.is_cancelled() {
        return Err(cancelled());
    }
    let mut process = RunningChild::spawn(command, input.is_some(), capture_stdout)?;
    let result = process.transfer(&mut input, cancellation, limits);
    if let Err(error) = result {
        // Closing stdin is always first, including partial writes and cancellation.
        process.stdin.take();
        let cleanup = process.stop(limits.terminate_grace);
        let mut message = error.to_string();
        if !process.output.stderr.bytes.is_empty() {
            message.push_str(&format!(": {}", process.output.stderr.text()));
        }
        if let Err(cleanup_error) = cleanup {
            message.push_str(&format!("; child cleanup failed: {cleanup_error}"));
            return Err(io::Error::other(message));
        }
        return Err(io::Error::new(error.kind(), message));
    }
    Ok(std::mem::take(&mut process.output))
}

struct RunningChild {
    child: Child,
    reaped: bool,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    output: Captured,
}

impl RunningChild {
    fn spawn(command: &mut Command, input: bool, capture_stdout: bool) -> io::Result<Self> {
        // A terminal's Ctrl+C reaches the parent handler. The parent then owns
        // termination/reaping instead of racing the child over the same signal.
        let mut child = command
            .env("LC_ALL", "C")
            .process_group(0)
            .stdin(if input { Stdio::piped() } else { Stdio::null() })
            .stdout(if capture_stdout { Stdio::piped() } else { Stdio::null() })
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| io::Error::new(error.kind(), format!(
                "cannot start pw-play: {error}; install the PipeWire command-line tools and ensure pw-play is executable on PATH"
            )))?;
        let process = Self {
            stdin: child.stdin.take(),
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            reaped: false,
            output: Captured::default(),
        };
        // The Drop guard already owns the child if any setup operation fails.
        if let Some(pipe) = &process.stdin {
            nonblocking(pipe)?;
        }
        if let Some(pipe) = &process.stdout {
            nonblocking(pipe)?;
        }
        if let Some(pipe) = &process.stderr {
            nonblocking(pipe)?;
        }
        Ok(process)
    }

    fn transfer(
        &mut self,
        input: &mut Option<Input<'_>>,
        cancellation: &Cancellation,
        limits: Limits,
    ) -> io::Result<()> {
        let repeating = matches!(input, Some(Input::Repeat { .. }));
        let started = Instant::now();
        let mut last_write = started;
        let mut written = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(cancelled());
            }
            if limits
                .timeout
                .is_some_and(|timeout| started.elapsed() >= timeout)
            {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pw-play exceeded its subprocess deadline",
                ));
            }
            if self.stdin.is_some() && last_write.elapsed() >= limits.no_progress {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pw-play made no progress reading audio input",
                ));
            }

            let bytes = match input {
                Some(Input::Finite(bytes)) => *bytes,
                Some(Input::Repeat { buffer, .. }) => *buffer,
                None => &[],
            };
            self.drain_output()?;
            if let Some(status) = self.try_wait()? {
                self.drain_output()?;
                if !status.success() {
                    return Err(io::Error::other(format!("pw-play exited with {status}")));
                }
                if repeating {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "continuous playback exited unexpectedly",
                    ));
                }
                if written != bytes.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "pw-play exited before all PCM bytes were sent",
                    ));
                }
                return Ok(());
            }

            if written == bytes.len() {
                self.stdin.take();
            } else if let Some(pipe) = self.stdin.as_mut() {
                let end = (written + IO_CHUNK).min(bytes.len());
                let mut progressed = false;
                match pipe.write(&bytes[written..end]) {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "pw-play accepted zero audio bytes",
                        ))
                    }
                    Ok(count) => {
                        progressed = true;
                        written += count;
                        last_write = Instant::now();
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => {
                        return Err(io::Error::new(
                            error.kind(),
                            format!("writing pw-play audio input failed: {error}"),
                        ))
                    }
                }
                if written == bytes.len() {
                    if repeating {
                        written = 0;
                    } else {
                        self.stdin.take();
                    }
                }
                if progressed {
                    if let Some(Input::Repeat { progress, .. }) = input {
                        progress();
                    }
                }
            }

            let mut wait = limits.poll_interval;
            if let Some(timeout) = limits.timeout {
                wait = wait.min(timeout.saturating_sub(started.elapsed()));
            }
            if self.stdin.is_some() {
                wait = wait.min(limits.no_progress.saturating_sub(last_write.elapsed()));
            }
            self.poll(wait)?;
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.reaped |= status.is_some();
        Ok(status)
    }

    fn drain_output(&mut self) -> io::Result<()> {
        self.output.stderr.drain(&mut self.stderr, STDERR_LIMIT)?;
        self.output.stdout.drain(&mut self.stdout, STDOUT_LIMIT)
    }

    fn poll(&self, duration: Duration) -> io::Result<()> {
        let mut fds = Vec::with_capacity(3);
        if let Some(pipe) = &self.stdin {
            fds.push(PollFd::new(pipe.as_fd(), PollFlags::POLLOUT));
        }
        if let Some(pipe) = &self.stdout {
            fds.push(PollFd::new(pipe.as_fd(), PollFlags::POLLIN));
        }
        if let Some(pipe) = &self.stderr {
            fds.push(PollFd::new(pipe.as_fd(), PollFlags::POLLIN));
        }
        // Round up sub-millisecond deadlines instead of spinning with poll(0).
        let milliseconds = duration.as_millis().saturating_add(1).min(100) as u16;
        match poll(&mut fds, milliseconds) {
            Ok(_) | Err(Errno::EINTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn stop(&mut self, grace: Duration) -> io::Result<()> {
        self.stdin.take();
        if self.try_wait()?.is_some() {
            return self.drain_output();
        }
        match kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(error) => return Err(error.into()),
        }
        let started = Instant::now();
        while started.elapsed() < grace {
            self.drain_output()?;
            if self.try_wait()?.is_some() {
                return self.drain_output();
            }
            self.poll(Duration::from_millis(20).min(grace.saturating_sub(started.elapsed())))?;
        }
        self.child.kill()?;
        self.child.wait()?;
        self.reaped = true;
        self.drain_output()
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        self.stdin.take();
        if !self.reaped {
            // Last-resort cleanup on setup failure or unwinding. Normal errors
            // use stop() first, with bounded graceful termination and drainage.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    let flags = OFlag::from_bits_truncate(fcntl(fd.as_raw_fd(), FcntlArg::F_GETFL)?);
    fcntl(fd.as_raw_fd(), FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "playback interrupted")
}

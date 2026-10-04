use std::fs;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

mod support;
use support::{wait_for_file, Fixture};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_speaker-keepalive"))
        .args(args)
        .env("PATH", "/nonexistent-speaker-keepalive-test-path")
        .output()
        .expect("run the compiled CLI")
}

fn with_backend(fixture: &Fixture) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_speaker-keepalive"));
    // Never allow tests to fall back to the real system pw-play.
    command.env("PATH", &fixture.directory);
    command
}

#[test]
fn one_shot_cli_checks_capabilities_and_sends_default_stereo_pcm() {
    let fixture = Fixture::new("ok");
    let output = with_backend(&fixture).arg("test").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let bytes = fs::read(fixture.directory.join("audio.pcm")).unwrap();
    assert_eq!(bytes.len(), 115_200);
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|s| f32::from_ne_bytes(s.try_into().unwrap()))
        .collect();
    assert!(samples.chunks_exact(2).all(|frame| frame[0] == frame[1]));
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    assert!((peak - 0.0005).abs() < 1e-10);
    assert_eq!(samples[0], 0.0);
    assert_eq!(*samples.last().unwrap(), 0.0);
    fixture.assert_reaped();
}

#[test]
fn missing_backend_is_an_actionable_startup_error() {
    let output = run(&["test"]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("cannot start pw-play"));
    assert!(error.contains("install the PipeWire command-line tools"));
    assert!(error.contains("PATH"));
}

#[test]
fn unsupported_capabilities_stop_before_playback() {
    for (case, expected) in [
        ("missing-flag", "does not advertise --raw"),
        ("missing-f32", "does not advertise f32"),
        ("probe-fail", "fake capability failure"),
        ("oversized-help", "exceeded 32 KiB"),
    ] {
        let fixture = Fixture::new(case);
        let output = with_backend(&fixture).arg("test").output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{case}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{case}: {error}");
        assert!(!fixture.directory.join("audio.args").exists());
        fixture.assert_reaped();
    }
}

#[test]
fn rejected_invocation_and_audio_failure_exit_nonzero() {
    for (case, expected) in [
        ("reject-playback", "fake unsupported invocation"),
        ("nonzero", "fake audio backend failed"),
    ] {
        let fixture = Fixture::new(case);
        let output = with_backend(&fixture).arg("test").output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("check that PipeWire is running"));
        fixture.assert_reaped();
    }
}

#[test]
fn real_sigint_cancels_probes_writes_and_playback_and_reaps_children() {
    for (case, marker) in [
        ("stall-probe", "ready"),
        ("ignore-term", "ready"),
        ("post-eof-hang", "eof"),
    ] {
        let fixture = Fixture::new(case);
        let mut child = ChildGuard(Some(
            with_backend(&fixture)
                .args(["test", "--verbose"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        wait_for_file(&fixture.directory.join(marker));
        let started = Instant::now();
        kill(
            Pid::from_raw(child.0.as_ref().unwrap().id() as i32),
            Signal::SIGINT,
        )
        .unwrap();
        while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{case}: CLI did not stop after SIGINT"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.0.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("child stopped and reaped"));
        assert!(started.elapsed() < Duration::from_secs(2), "{case}");
        if case == "ignore-term" {
            assert!(started.elapsed() >= Duration::from_millis(500));
            assert!(fixture.directory.join("term").exists());
        }
        fixture.assert_reaped();
    }
}

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn help_explains_pcm_amplitude_and_defaults() {
    for command in ["run", "test"] {
        let output = run(&[command, "--help"]);
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("PCM peak amplitude relative to full scale"));
        assert!(help.contains("not device volume"));
        assert!(help.contains("0.0005"));
        assert!(help.contains("--verbose"));
    }
}

#[test]
fn invalid_arguments_fail_before_backend_execution() {
    for (args, expected) in [
        (vec!["test", "--volume", "NaN"], "--volume"),
        (vec!["run", "--interval", "0"], "--interval"),
        (
            vec!["run", "--mode", "silence", "--duration", "300"],
            "cannot be combined",
        ),
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(expected));
        assert!(!error.contains("not implemented yet"));
    }
}

fn start_run(fixture: &Fixture, interval: &str, verbose: bool) -> ChildGuard {
    let mut command = with_backend(fixture);
    command.args(["run", "--interval", interval]);
    if verbose {
        command.arg("--verbose");
    }
    ChildGuard(Some(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ))
}

fn stop_run(child: &mut ChildGuard) -> Output {
    let started = Instant::now();
    kill(
        Pid::from_raw(child.0.as_ref().unwrap().id() as i32),
        Signal::SIGINT,
    )
    .unwrap();
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "periodic run failed to stop"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn wait_for_lines(fixture: &Fixture, name: &str, count: usize) -> Vec<String> {
    let started = Instant::now();
    loop {
        let text = fs::read_to_string(fixture.directory.join(name)).unwrap_or_default();
        let lines: Vec<_> = text.lines().map(str::to_owned).collect();
        if lines.len() >= count {
            return lines;
        }
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "waiting for {name}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn periodic_reuses_pcm_probes_once_and_never_overlaps() {
    let fixture = Fixture::new("ok");
    let mut child = start_run(&fixture, "1", false);
    let hashes = wait_for_lines(&fixture, "hashes", 3);
    let output = stop_run(&mut child);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(hashes.iter().all(|hash| hash == &hashes[0]));
    let calls = fs::read_to_string(fixture.directory.join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "--help").count(), 1);
    assert_eq!(
        calls
            .lines()
            .filter(|line| *line == "--list-formats")
            .count(),
        1
    );
    let starts: Vec<f64> = wait_for_lines(&fixture, "starts", 3)
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(starts.windows(2).all(|pair| pair[1] - pair[0] >= 0.8));
    assert!(!fixture.directory.join("overlap").exists());
    fixture.assert_reaped();
}

#[test]
fn periodic_sigint_stops_long_idle_wait_and_active_playback() {
    for (case, marker) in [("ok", "finishes"), ("ignore-term", "ready")] {
        let fixture = Fixture::new(case);
        let mut child = start_run(&fixture, "3600", true);
        wait_for_file(&fixture.directory.join(marker));
        // Let the successful backend be reaped and enter its hour-long wait.
        if case == "ok" {
            std::thread::sleep(Duration::from_millis(200));
        }
        let output = stop_run(&mut child);
        assert!(String::from_utf8_lossy(&output.stderr).contains("child stopped and reaped"));
        fixture.assert_reaped();
    }
}

#[test]
fn periodic_overrun_leaves_a_full_interval_before_next_child() {
    let fixture = Fixture::new("overrun");
    let mut child = start_run(&fixture, "1", false);
    let starts = wait_for_lines(&fixture, "starts", 2);
    let finishes = wait_for_lines(&fixture, "finishes", 1);
    stop_run(&mut child);
    assert!(starts[1].parse::<f64>().unwrap() - finishes[0].parse::<f64>().unwrap() >= 1.0);
    assert!(!fixture.directory.join("overlap").exists());
    fixture.assert_reaped();
}

#[test]
fn periodic_failure_retries_then_reports_recovery() {
    let fixture = Fixture::new("nonzero");
    let mut child = start_run(&fixture, "1", false);
    wait_for_lines(&fixture, "finishes", 2);
    fs::write(fixture.directory.join("case"), "ok").unwrap();
    wait_for_lines(&fixture, "finishes", 3);
    std::thread::sleep(Duration::from_millis(150));
    let output = stop_run(&mut child);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("fake audio backend failed").count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains("playback recovered"), "{stderr}");
    assert!(!fixture.directory.join("overlap").exists());
    fixture.assert_reaped();
}

fn start_silence(fixture: &Fixture) -> ChildGuard {
    ChildGuard(Some(
        with_backend(fixture)
            .args(["run", "--mode", "silence"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ))
}

#[test]
fn silence_uses_one_persistent_zero_stream_and_stops_under_backpressure() {
    for (case, marker) in [("silence-slow", "zero-verified"), ("ignore-term", "ready")] {
        let fixture = Fixture::new(case);
        let mut child = start_silence(&fixture);
        wait_for_file(&fixture.directory.join(marker));
        if case == "silence-slow" {
            std::thread::sleep(Duration::from_millis(200));
            assert!(!fixture.directory.join("nonzero").exists());
        }
        assert!(child.0.as_mut().unwrap().try_wait().unwrap().is_none());
        let output = stop_run(&mut child);
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let args = fs::read_to_string(fixture.directory.join("audio.args")).unwrap();
        assert!(args.contains("--properties\n{\"application.id\":\"speaker-keepalive\",\"application.name\":\"speaker-keepalive\"}"));
        assert_eq!(
            fs::read_to_string(fixture.directory.join("starts"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(!fixture.directory.join("audio.pcm").exists());
        fixture.assert_reaped();
    }
}

#[test]
fn silence_restart_wait_is_interruptible() {
    let fixture = Fixture::new("reject-playback");
    let mut child = start_silence(&fixture);
    wait_for_file(&fixture.directory.join("finishes"));
    std::thread::sleep(Duration::from_millis(200));
    let output = stop_run(&mut child);
    assert!(String::from_utf8_lossy(&output.stderr).contains("restarting silence in 30 seconds"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    fixture.assert_reaped();
}

#[test]
fn silence_restart_obeys_real_thirty_second_delay_and_reaps_old_child() {
    let fixture = Fixture::new("early-exit");
    let mut child = start_silence(&fixture);
    let finishes = wait_for_lines(&fixture, "finishes", 1);
    // The next invocation will stay alive, proving the restart can resume streaming.
    fs::write(fixture.directory.join("case"), "silence-slow").unwrap();
    let started = Instant::now();
    let starts = loop {
        let content = fs::read_to_string(fixture.directory.join("starts")).unwrap();
        let lines: Vec<f64> = content.lines().map(|s| s.parse().unwrap()).collect();
        if lines.len() >= 2 {
            break lines;
        }
        assert!(
            started.elapsed() < Duration::from_secs(35),
            "silence stream did not restart"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(starts[1] - finishes[0].parse::<f64>().unwrap() >= 30.0);
    wait_for_file(&fixture.directory.join("zero-verified"));
    stop_run(&mut child);
    assert!(!fixture.directory.join("overlap").exists());
    let calls = fs::read_to_string(fixture.directory.join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "--help").count(), 1);
    fixture.assert_reaped();
}

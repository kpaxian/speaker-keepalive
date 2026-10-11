//! Bounded PipeWire playback for pulses, continuous tones, and silence.

mod continuous;
mod diagnostics;
mod periodic;
mod process;

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::audio::{PulseParameters, ToneParameters, CHANNELS, SAMPLE_RATE};
use process::{Cancellation, Limits};

const STREAM_PROPERTIES: &str =
    r#"{"application.id":"speaker-keepalive","application.name":"speaker-keepalive"}"#;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PLAYBACK_SLACK: Duration = Duration::from_secs(10);

pub fn play_once(samples: &[f32], verbose: bool) -> io::Result<()> {
    let cancellation = Cancellation::install()?;
    let backend = Backend::new("pw-play");
    backend.check(&cancellation)?;
    let pcm = encode_pcm(samples);
    let duration =
        Duration::from_secs_f64(samples.len() as f64 / CHANNELS as f64 / f64::from(SAMPLE_RATE));
    if verbose {
        eprintln!(
            "playing one pulse: {} frames, {:.0} ms, {SAMPLE_RATE} Hz, {CHANNELS} channels",
            samples.len() / CHANNELS,
            duration.as_secs_f64() * 1000.0,
        );
    }
    let started = Instant::now();
    let output = backend.play(&pcm, duration, &cancellation)?;
    if verbose {
        if !output.stderr.bytes.is_empty() {
            eprintln!("pw-play: {}", output.stderr.text());
        }
        eprintln!(
            "pw-play completed in {:.3} s; speaker delivery/effectiveness is not verified",
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// Raw SPA_AUDIO_FORMAT_F32 is native endian. Keep this byte buffer for reuse
/// across periodic attempts; do not apply further amplitude scaling in the subprocess.
fn encode_pcm(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_ne_bytes())
        .collect()
}

struct Backend {
    program: PathBuf,
}

impl Backend {
    fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// Capability checks only: these commands do not open an audio stream.
    fn check(&self, cancellation: &Cancellation) -> io::Result<()> {
        let help = self.probe("--help", cancellation)?;
        for required in [
            "--properties",
            "--raw",
            "--format",
            "--rate",
            "--channels",
            "--volume",
            "--list-formats",
        ] {
            if !help
                .split_whitespace()
                .any(|word| word.trim_end_matches(',') == required)
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("installed pw-play does not advertise {required}; install compatible PipeWire command-line tools"),
                ));
            }
        }
        let formats = self.probe("--list-formats", cancellation)?;
        if !formats.lines().any(|line| line.trim() == "f32") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "installed pw-play does not advertise f32 PCM; install compatible PipeWire command-line tools",
            ));
        }
        Ok(())
    }

    fn probe(&self, argument: &str, cancellation: &Cancellation) -> io::Result<String> {
        let output = process::execute(
            Command::new(&self.program).arg(argument),
            None,
            true,
            cancellation,
            Limits::with_timeout(PROBE_TIMEOUT),
        )
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("pw-play capability check {argument} failed: {error}"),
            )
        })?;
        if output.stdout.truncated {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pw-play capability output exceeded 32 KiB",
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout.bytes).into_owned())
    }

    fn playback_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args([
            "--raw",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--volume",
            "1",
            "--properties",
            STREAM_PROPERTIES,
            "-",
        ]);
        command
    }

    fn play(
        &self,
        pcm: &[u8],
        duration: Duration,
        cancellation: &Cancellation,
    ) -> io::Result<process::Captured> {
        process::execute(
            &mut self.playback_command(),
            Some(pcm),
            false,
            cancellation,
            Limits::with_timeout(duration + PLAYBACK_SLACK),
        ).map_err(|error| {
            io::Error::new(error.kind(), format!("pw-play playback failed: {error}; check that PipeWire is running and the intended default output is available"))
        })
    }
}

pub fn run_pulses(signal: &PulseParameters, interval: Duration, verbose: bool) -> io::Result<()> {
    let cancellation = Cancellation::install()?;
    let backend = Backend::new("pw-play");
    backend.check(&cancellation)?;
    let pcm = encode_pcm(&signal.generate());
    periodic::run(
        &backend,
        &pcm,
        signal.duration(),
        interval,
        verbose,
        &cancellation,
    )
}

pub fn run_silence(verbose: bool) -> io::Result<()> {
    let cancellation = Cancellation::install()?;
    let backend = Backend::new("pw-play");
    backend.check(&cancellation)?;
    continuous::run(&backend, None, verbose, &cancellation)
}

pub fn run_tone(signal: ToneParameters, verbose: bool) -> io::Result<()> {
    let cancellation = Cancellation::install()?;
    let backend = Backend::new("pw-play");
    backend.check(&cancellation)?;
    continuous::run(&backend, Some(signal), verbose, &cancellation)
}

#[cfg(test)]
mod tests;

use std::io;
use std::time::{Duration, Instant};

use crate::audio::ToneParameters;

use super::{diagnostics::Diagnostics, process, Backend, Cancellation, Limits};

// 2048 stereo f32 frames. Positive zero has all-zero bits on either endianness.
const ZERO_PCM: [u8; 16 * 1024] = [0; 16 * 1024];
const RESTART_DELAY: Duration = Duration::from_secs(30);

pub(super) fn run(
    backend: &Backend,
    tone: Option<ToneParameters>,
    verbose: bool,
    cancellation: &Cancellation,
) -> io::Result<()> {
    let mode = if tone.is_some() { "tone" } else { "silence" };
    let mut limits = Limits::with_timeout(Duration::ZERO);
    limits.timeout = None;
    let anchor = Instant::now();
    let mut diagnostics = Diagnostics::default();
    loop {
        if verbose {
            eprintln!("starting continuous {mode} stream: {tone:?}");
        }
        let started = Instant::now();
        let mut reported_progress = false;
        // Fresh phase/fade for each replacement stream; no resets between chunks.
        let mut generator = tone.map(ToneParameters::generator);
        let mut buffer = [0_u8; 16 * 1024];
        let mut refill = |bytes: &mut [u8]| generator.as_mut().unwrap().fill_pcm(bytes);
        let mut progress = || {
            // Pipe input progress does not prove delivery to the speaker.
            if !reported_progress && started.elapsed() >= limits.no_progress {
                reported_progress = true;
                if let Some(message) = diagnostics.record(anchor.elapsed(), None, verbose) {
                    eprintln!(
                        "{message}; {mode} input has resumed, speaker delivery is unverified"
                    );
                }
                if verbose {
                    eprintln!("continuous {mode} input is progressing");
                }
            }
        };
        let input = if tone.is_some() {
            process::Input::Generated {
                buffer: &mut buffer,
                refill: &mut refill,
                progress: &mut progress,
            }
        } else {
            process::Input::Repeat {
                buffer: &ZERO_PCM,
                progress: &mut progress,
            }
        };
        let error = process::execute_input(
            &mut backend.playback_command(),
            Some(input),
            false,
            cancellation,
            limits,
        )
        .expect_err("a continuous stream cannot complete normally");
        if error.kind() == io::ErrorKind::Interrupted || cancellation.is_cancelled() {
            return Err(error);
        }
        if let Some(message) = diagnostics.record(
            anchor.elapsed(),
            Some(format!("{:?}: {error}", error.kind())),
            verbose,
        ) {
            eprintln!("{message}; restarting {mode} in 30 seconds");
        }
        // execute_input has terminated/reaped the old child before this delay starts.
        cancellation.wait_until(Instant::now() + RESTART_DELAY)?;
    }
}

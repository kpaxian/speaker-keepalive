use std::io;
use std::time::{Duration, Instant};

use super::{diagnostics::Diagnostics, process, Backend, Cancellation, Limits};

// 2048 stereo f32 frames. Positive zero has all-zero bits on either endianness.
const ZERO_PCM: [u8; 16 * 1024] = [0; 16 * 1024];
const RESTART_DELAY: Duration = Duration::from_secs(30);

pub(super) fn run(backend: &Backend, verbose: bool, cancellation: &Cancellation) -> io::Result<()> {
    let mut limits = Limits::with_timeout(Duration::ZERO);
    limits.timeout = None;
    let anchor = Instant::now();
    let mut diagnostics = Diagnostics::default();
    loop {
        if verbose {
            eprintln!("starting continuous silence stream");
        }
        let started = Instant::now();
        let mut reported_progress = false;
        let error = process::execute_input(
            &mut backend.playback_command(),
            Some(process::Input::Repeat {
                buffer: &ZERO_PCM,
                progress: &mut || {
                    // Require sustained input progress before resetting failure suppression.
                    // This is transport recovery only, never proof of speaker delivery.
                    if !reported_progress && started.elapsed() >= limits.no_progress {
                        reported_progress = true;
                        if let Some(message) = diagnostics.record(anchor.elapsed(), None, verbose) {
                            eprintln!("{message}; silence input has resumed, speaker delivery is unverified");
                        }
                        if verbose { eprintln!("continuous silence input is progressing"); }
                    }
                },
            }),
            false,
            cancellation,
            limits,
        ).expect_err("a repeating stream cannot complete normally");
        if error.kind() == io::ErrorKind::Interrupted || cancellation.is_cancelled() {
            return Err(error);
        }
        if let Some(message) = diagnostics.record(
            anchor.elapsed(),
            Some(format!("{:?}: {error}", error.kind())),
            verbose,
        ) {
            eprintln!("{message}; restarting silence in 30 seconds");
        }
        // execute_input has terminated/reaped the old child before this delay starts.
        cancellation.wait_until(Instant::now() + RESTART_DELAY)?;
    }
}

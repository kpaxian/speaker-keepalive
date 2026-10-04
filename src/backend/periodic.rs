//! Scheduling is pure monotonic arithmetic; playback owns exactly one child.
use std::io;
use std::time::{Duration, Instant};

use super::{diagnostics::Diagnostics, process::Cancellation, Backend};

pub(super) fn run(
    backend: &Backend,
    pcm: &[u8],
    duration: Duration,
    interval: Duration,
    verbose: bool,
    cancellation: &Cancellation,
) -> io::Result<()> {
    let anchor = Instant::now();
    let mut deadline = Duration::ZERO;
    let mut diagnostics = Diagnostics::default();
    loop {
        cancellation.wait_until(anchor + deadline)?;
        let started = anchor.elapsed();
        // A late wake may consume one slot, but never replay missed slots.
        if started >= deadline + interval {
            deadline = future_slot(started, interval);
            continue;
        }
        if verbose {
            eprintln!("starting pulse at {:.3} s", started.as_secs_f64());
        }
        let result = backend.play(pcm, duration, cancellation);
        if let Err(error) = &result {
            // Cleanup errors remain failures even when cancellation triggered them.
            if error.kind() == io::ErrorKind::Interrupted || cancellation.is_cancelled() {
                return Err(io::Error::new(error.kind(), error.to_string()));
            }
        }
        let finished = anchor.elapsed();
        let timeout = result
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::TimedOut);
        if let Some(message) = diagnostics.record(
            finished,
            result
                .as_ref()
                .err()
                .map(|e| format!("{:?}: {e}", e.kind())),
            verbose,
        ) {
            eprintln!("{message}; continuing on pulse schedule");
        }
        if verbose {
            if let Ok(output) = &result {
                if !output.stderr.bytes.is_empty() {
                    eprintln!("pw-play: {}", output.stderr.text());
                }
                eprintln!(
                    "pulse completed in {:.3} s",
                    (finished - started).as_secs_f64()
                );
            }
        }
        deadline = next_deadline(deadline, finished, interval, timeout);
    }
}

fn future_slot(now: Duration, interval: Duration) -> Duration {
    let remainder = Duration::from_nanos((now.as_nanos() % interval.as_nanos()) as u64);
    now + (interval - remainder)
}

fn next_deadline(
    previous: Duration,
    finished: Duration,
    interval: Duration,
    timeout: bool,
) -> Duration {
    if timeout || finished >= previous + interval {
        // Round the cooldown up to an anchored slot (inclusive at exact boundary).
        let earliest = finished + interval;
        let remainder = Duration::from_nanos((earliest.as_nanos() % interval.as_nanos()) as u64);
        if remainder.is_zero() {
            earliest
        } else {
            earliest + interval - remainder
        }
    } else {
        future_slot(finished, interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn playback_duration_does_not_accumulate_drift() {
        let mut deadline = Duration::ZERO;
        for expected in 1..100 {
            deadline = next_deadline(deadline, deadline + ms(300), ms(1000), false);
            assert_eq!(deadline, ms(expected * 1000));
        }
    }

    #[test]
    fn overruns_and_timeouts_wait_a_full_interval_on_the_grid() {
        for (finished, timeout, expected) in [
            (999, false, 1000),
            (1000, false, 2000),
            (1300, false, 3000),
            (200, true, 2000),
            (1_000_300, false, 1_002_000),
        ] {
            assert_eq!(
                next_deadline(ms(0), ms(finished), ms(1000), timeout),
                ms(expected)
            );
        }
    }

    #[test]
    fn delayed_wakes_skip_to_a_future_slot_without_backlog() {
        assert_eq!(future_slot(ms(1000), ms(1000)), ms(2000));
        assert_eq!(future_slot(ms(86_400_123), ms(1000)), ms(86_401_000));
        assert_eq!(next_deadline(ms(1000), ms(1400), ms(1000), false), ms(2000));
    }

    #[test]
    fn diagnostics_report_changes_minute_summaries_and_recovery() {
        let mut logs = Diagnostics::default();
        assert!(logs.record(ms(0), None, false).is_none());
        assert!(logs.record(ms(0), Some("A".into()), false).is_some());
        assert!(logs.record(ms(59_999), Some("A".into()), false).is_none());
        assert!(logs
            .record(ms(60_000), Some("A".into()), false)
            .unwrap()
            .contains("1 repeated"));
        assert!(logs.record(ms(60_001), Some("B".into()), false).is_some());
        assert!(logs.record(ms(60_002), Some("B".into()), true).is_some());
        assert!(logs
            .record(ms(60_003), None, false)
            .unwrap()
            .contains("recovered"));
        assert!(logs.record(ms(60_004), None, false).is_none());
    }

    #[test]
    fn idle_cancellation_wakes_and_precancellation_is_not_lost() {
        let cancel = Cancellation::default();
        let other = cancel.clone();
        let waiter = std::thread::spawn(move || {
            other.wait_until(Instant::now() + Duration::from_secs(3600))
        });
        cancel.cancel();
        assert_eq!(
            waiter.join().unwrap().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(
            cancel.wait_until(Instant::now()).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
}

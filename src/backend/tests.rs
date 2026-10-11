use super::*;
use std::fs;
use std::thread;

#[path = "../../tests/support/mod.rs"]
mod support;
use support::{wait_for_file, Fixture};

fn limits() -> Limits {
    Limits {
        timeout: Some(Duration::from_secs(3)),
        no_progress: Duration::from_millis(300),
        poll_interval: Duration::from_millis(10),
        terminate_grace: Duration::from_millis(100),
    }
}

#[test]
fn native_float_encoding_preserves_samples_without_scaling() {
    let samples = [0.0_f32, -0.0, 0.0005, -0.05];
    let bytes = encode_pcm(&samples);
    assert_eq!(bytes.len(), 16);
    for (sample, encoded) in samples.iter().zip(bytes.chunks_exact(4)) {
        assert_eq!(
            f32::from_ne_bytes(encoded.try_into().unwrap()).to_bits(),
            sample.to_bits()
        );
    }
}

#[test]
fn supported_backend_gets_exact_raw_arguments_and_pcm() {
    let fixture = Fixture::new("ok");
    let backend = Backend::new(&fixture.program);
    let cancel = Cancellation::default();
    backend.check(&cancel).unwrap();
    let pcm = encode_pcm(&PulseParameters::new(300, 100.0, 0.0005).unwrap().generate());
    backend
        .play(&pcm, Duration::from_millis(300), &cancel)
        .unwrap();
    assert_eq!(fs::read(fixture.directory.join("audio.pcm")).unwrap(), pcm);
    assert_eq!(
        fs::read_to_string(fixture.directory.join("audio.args")).unwrap(),
        format!("--raw\n--format\nf32\n--rate\n48000\n--channels\n2\n--volume\n1\n--properties\n{STREAM_PROPERTIES}\n-")
    );
    assert_eq!(
        fs::read_to_string(fixture.directory.join("calls"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    fixture.assert_reaped();
}

#[test]
fn slow_reader_receives_every_byte_across_partial_writes() {
    let fixture = Fixture::new("slow");
    let input: Vec<u8> = (0..=255).cycle().take(512 * 1024).collect();
    process::execute(
        &mut Command::new(&fixture.program),
        Some(&input),
        false,
        &Cancellation::default(),
        limits(),
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.directory.join("audio.pcm")).unwrap(),
        input
    );
    fixture.assert_reaped();
}

#[test]
fn stderr_flood_is_drained_but_capture_stays_bounded() {
    let fixture = Fixture::new("flood");
    let input = vec![0; 128 * 1024];
    let output = process::execute(
        &mut Command::new(&fixture.program),
        Some(&input),
        false,
        &Cancellation::default(),
        limits(),
    )
    .unwrap();
    assert_eq!(output.stderr.bytes.len(), 8192);
    assert!(output.stderr.truncated);
    assert!(output.stderr.text().ends_with("[truncated]"));
    assert_eq!(
        fs::read(fixture.directory.join("audio.pcm")).unwrap(),
        input
    );
    fixture.assert_reaped();
}

#[test]
fn nonzero_exit_preserves_status_and_stderr() {
    let fixture = Fixture::new("nonzero");
    let error = process::execute(
        &mut Command::new(&fixture.program),
        Some(&[0; 8]),
        false,
        &Cancellation::default(),
        limits(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("exit status: 7"), "{error}");
    assert!(
        error.to_string().contains("fake audio backend failed"),
        "{error}"
    );
    fixture.assert_reaped();
}

#[test]
fn early_success_exit_and_broken_pipe_are_failures() {
    for case in ["early-exit", "close-input"] {
        let fixture = Fixture::new(case);
        let error = process::execute(
            &mut Command::new(&fixture.program),
            Some(&vec![0; 512 * 1024]),
            false,
            &Cancellation::default(),
            limits(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof
            ),
            "{case}: {error}"
        );
        fixture.assert_reaped();
    }
}

#[test]
fn no_progress_watchdog_works_without_an_overall_deadline() {
    let fixture = Fixture::new("stall");
    let mut bounds = limits();
    bounds.timeout = None;
    let started = Instant::now();
    let error = process::execute(
        &mut Command::new(&fixture.program),
        Some(&vec![0; 512 * 1024]),
        false,
        &Cancellation::default(),
        bounds,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("no progress"));
    assert!(started.elapsed() < Duration::from_secs(2));
    fixture.assert_reaped();
}

#[test]
fn deadline_bounds_waiting_for_exit_after_eof() {
    let fixture = Fixture::new("post-eof-hang");
    let mut bounds = limits();
    bounds.timeout = Some(Duration::from_millis(300));
    let started = Instant::now();
    let error = process::execute(
        &mut Command::new(&fixture.program),
        Some(&[0; 8]),
        false,
        &Cancellation::default(),
        bounds,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("deadline"));
    assert!(fixture.directory.join("eof").exists());
    assert!(started.elapsed() < Duration::from_secs(2));
    fixture.assert_reaped();
}

#[test]
fn stalled_capability_probe_times_out_and_is_reaped() {
    let fixture = Fixture::new("stall-probe");
    let mut bounds = limits();
    bounds.timeout = Some(Duration::from_millis(300));
    let error = process::execute(
        Command::new(&fixture.program).arg("--help"),
        None,
        true,
        &Cancellation::default(),
        bounds,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    fixture.assert_reaped();
}

#[test]
fn cancellation_handles_blocked_writes_eof_wait_and_stderr_flood() {
    for (case, marker) in [
        ("stall", "ready"),
        ("post-eof-hang", "eof"),
        ("flood-stall", "ready"),
        ("closed-output", "ready"),
    ] {
        let fixture = Fixture::new(case);
        let cancel = Cancellation::default();
        let trigger = cancel.clone();
        let ready = fixture.directory.join(marker);
        let worker = thread::spawn(move || {
            wait_for_file(&ready);
            trigger.cancel();
            Instant::now()
        });
        let error = process::execute(
            &mut Command::new(&fixture.program),
            Some(&vec![0; 128 * 1024]),
            false,
            &cancel,
            limits(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted, "{case}: {error}");
        assert!(worker.join().unwrap().elapsed() < Duration::from_secs(2));
        assert!(error.to_string().len() < 8400);
        fixture.assert_reaped();
    }
}

#[test]
fn cancellation_escalates_when_child_ignores_sigterm() {
    let fixture = Fixture::new("ignore-term");
    let cancel = Cancellation::default();
    let trigger = cancel.clone();
    let ready = fixture.directory.join("ready");
    let worker = thread::spawn(move || {
        wait_for_file(&ready);
        trigger.cancel();
        Instant::now()
    });
    let error = process::execute(
        &mut Command::new(&fixture.program),
        Some(&vec![0; 128 * 1024]),
        false,
        &cancel,
        limits(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    let elapsed = worker.join().unwrap().elapsed();
    assert!(elapsed >= Duration::from_millis(100));
    assert!(elapsed < Duration::from_secs(2));
    assert!(fixture.directory.join("term").exists());
    fixture.assert_reaped();
}

#[test]
fn already_cancelled_request_does_not_spawn() {
    let fixture = Fixture::new("ok");
    let cancel = Cancellation::default();
    cancel.cancel();
    let error = process::execute(
        &mut Command::new(&fixture.program),
        Some(&[0; 8]),
        false,
        &cancel,
        limits(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(!fixture.directory.join("pids").exists());
}

#[test]
fn repeating_input_watchdog_bounds_stalls_and_noisy_stalls() {
    for case in ["stall", "flood-stall"] {
        let fixture = Fixture::new(case);
        let mut bounds = limits();
        bounds.timeout = None;
        let started = Instant::now();
        let error = process::execute_input(
            &mut Command::new(&fixture.program),
            Some(process::Input::Repeat {
                buffer: &[0; 16384],
                progress: &mut || {},
            }),
            false,
            &Cancellation::default(),
            bounds,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        assert!(error.to_string().contains("no progress"));
        assert!(error.to_string().len() < 8400);
        assert!(started.elapsed() < Duration::from_secs(2));
        fixture.assert_reaped();
    }
}

#[test]
fn repeating_input_rejects_empty_buffers_and_unexpected_success_exit() {
    let fixture = Fixture::new("early-exit");
    for (buffer, expected) in [
        (&[][..], io::ErrorKind::InvalidInput),
        (&[0; 16384][..], io::ErrorKind::UnexpectedEof),
    ] {
        let error = process::execute_input(
            &mut Command::new(&fixture.program),
            Some(process::Input::Repeat {
                buffer,
                progress: &mut || {},
            }),
            false,
            &Cancellation::default(),
            limits(),
        )
        .unwrap_err();
        // An exit may race a pipe write, which is also a stream failure.
        assert!(
            error.kind() == expected
                || (expected == io::ErrorKind::UnexpectedEof
                    && error.kind() == io::ErrorKind::BrokenPipe),
            "{error}"
        );
        if buffer.is_empty() {
            assert!(!fixture.directory.join("pids").exists());
        }
    }
    fixture.assert_reaped();
}

#[test]
fn generated_input_bounds_stalls_diagnostics_and_early_exits() {
    for case in ["stall", "flood-stall", "early-exit"] {
        let fixture = Fixture::new(case);
        let mut bounds = limits();
        bounds.timeout = None;
        let mut buffer = [0; 16384];
        let mut generator = crate::audio::ToneParameters::new(100.0, 0.0005)
            .unwrap()
            .generator();
        let started = Instant::now();
        let error = process::execute_input(
            &mut Command::new(&fixture.program),
            Some(process::Input::Generated {
                buffer: &mut buffer,
                refill: &mut |bytes| generator.fill_pcm(bytes),
                progress: &mut || {},
            }),
            false,
            &Cancellation::default(),
            bounds,
        )
        .unwrap_err();
        if case == "early-exit" {
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
                ),
                "{error}"
            );
        } else {
            assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
            assert!(error.to_string().contains("no progress"));
        }
        assert!(error.to_string().len() < 8400);
        assert!(started.elapsed() < Duration::from_secs(2));
        fixture.assert_reaped();
    }
}

use std::time::Duration;

use clap::{error::ErrorKind, Args, Parser, Subcommand, ValueEnum};

use crate::audio::{
    PulseParameters, ToneParameters, DEFAULT_AMPLITUDE, DEFAULT_DURATION_MS, DEFAULT_FREQUENCY_HZ,
};

const DEFAULT_INTERVAL_SECONDS: u64 = 30;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Experimental Bluetooth speaker keepalive for PipeWire"
)]
pub struct Cli {
    /// Show playback parameters, results, and stream diagnostics
    #[arg(long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run periodic pulses, a continuous tone, or digital silence until interrupted
    Run {
        #[arg(long, value_enum, default_value_t = Mode::Pulse)]
        mode: Mode,
        /// Seconds between pulse starts (integer 1..=3600; default: 30)
        #[arg(long, allow_hyphen_values = true)]
        interval: Option<u64>,
        #[command(flatten)]
        signal: SignalArgs,
    },
    /// Play one pulse and exit
    Test {
        #[command(flatten)]
        signal: SignalArgs,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    Pulse,
    Silence,
    Tone,
}

#[derive(Debug, Args)]
struct SignalArgs {
    /// Pulse duration in milliseconds (integer 20..=5000; default: 300)
    #[arg(long, allow_hyphen_values = true)]
    duration: Option<u64>,
    /// Sine frequency in Hz (20..=1000; default: 100)
    #[arg(long, allow_hyphen_values = true)]
    frequency: Option<f64>,
    /// PCM peak amplitude relative to full scale, not device volume (0..=0.05; default: 0.0005)
    #[arg(long, allow_hyphen_values = true)]
    volume: Option<f64>,
}

impl SignalArgs {
    fn resolve(self) -> Result<PulseParameters, clap::Error> {
        PulseParameters::new(
            self.duration.unwrap_or(DEFAULT_DURATION_MS),
            self.frequency.unwrap_or(DEFAULT_FREQUENCY_HZ),
            self.volume.unwrap_or(DEFAULT_AMPLITUDE),
        )
        .map_err(|message| clap::Error::raw(ErrorKind::ValueValidation, message))
    }
}

/// All values have been validated before execution reaches the backend.
#[derive(Debug, PartialEq)]
pub enum Operation {
    Test(PulseParameters),
    RunPulse {
        interval: Duration,
        signal: PulseParameters,
    },
    RunSilence,
    RunTone(ToneParameters),
}

impl Cli {
    pub fn into_operation(self) -> Result<Operation, clap::Error> {
        match self.command {
            Command::Run {
                mode: Mode::Tone,
                interval,
                signal,
            } => {
                if interval.is_some() || signal.duration.is_some() {
                    return Err(clap::Error::raw(ErrorKind::ArgumentConflict,
                        "--mode tone cannot be combined with --interval or --duration; tone streams continuously"));
                }
                Ok(Operation::RunTone(
                    ToneParameters::new(
                        signal.frequency.unwrap_or(DEFAULT_FREQUENCY_HZ),
                        signal.volume.unwrap_or(DEFAULT_AMPLITUDE),
                    )
                    .map_err(|message| clap::Error::raw(ErrorKind::ValueValidation, message))?,
                ))
            }
            Command::Test { signal } => Ok(Operation::Test(signal.resolve()?)),
            Command::Run {
                mode: Mode::Silence,
                interval,
                signal,
            } => {
                if interval.is_some()
                    || signal.duration.is_some()
                    || signal.frequency.is_some()
                    || signal.volume.is_some()
                {
                    return Err(clap::Error::raw(
                        ErrorKind::ArgumentConflict,
                        "--mode silence cannot be combined with --interval, --duration, --frequency, or --volume; silence always streams zero-valued PCM",
                    ));
                }
                Ok(Operation::RunSilence)
            }
            Command::Run {
                mode: Mode::Pulse,
                interval,
                signal,
            } => {
                let interval = interval.unwrap_or(DEFAULT_INTERVAL_SECONDS);
                if !(1..=3600).contains(&interval) {
                    return Err(clap::Error::raw(
                        ErrorKind::ValueValidation,
                        "--interval must be an integer between 1 and 3600 seconds",
                    ));
                }
                let interval = Duration::from_secs(interval);
                let signal = signal.resolve()?;
                if signal.duration() >= interval {
                    return Err(clap::Error::raw(
                        ErrorKind::ValueValidation,
                        "--duration must be shorter than --interval (duration is in milliseconds; interval is in seconds)",
                    ));
                }
                Ok(Operation::RunPulse { interval, signal })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Operation, clap::Error> {
        Cli::try_parse_from(std::iter::once("speaker-keepalive").chain(args.iter().copied()))?
            .into_operation()
    }

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn tone_defaults_overrides_and_conflicts_are_validated() {
        assert_eq!(
            parse(&["run", "--mode", "tone"]).unwrap(),
            Operation::RunTone(ToneParameters::new(100.0, 0.0005).unwrap())
        );
        for (frequency, amplitude) in [("20", "0"), ("1000", "0.05"), ("137.25", "0.001")] {
            assert_eq!(
                parse(&[
                    "run",
                    "--mode",
                    "tone",
                    "--frequency",
                    frequency,
                    "--volume",
                    amplitude
                ])
                .unwrap(),
                Operation::RunTone(
                    ToneParameters::new(frequency.parse().unwrap(), amplitude.parse().unwrap())
                        .unwrap()
                )
            );
        }
        for (flag, value) in [("--interval", "30"), ("--duration", "300")] {
            assert_eq!(
                parse(&["run", "--mode", "tone", flag, value])
                    .unwrap_err()
                    .kind(),
                ErrorKind::ArgumentConflict
            );
        }
        for (flag, values) in [
            ("--frequency", &["19", "1001", "NaN", "inf", "-inf"][..]),
            ("--volume", &["-0.001", "0.051", "NaN", "inf", "-inf"][..]),
        ] {
            for value in values {
                assert_eq!(
                    parse(&["run", "--mode", "tone", flag, value])
                        .unwrap_err()
                        .kind(),
                    ErrorKind::ValueValidation
                );
            }
        }
    }

    #[test]
    fn default_run_and_test_use_specification_defaults() {
        let signal = PulseParameters::new(300, 100.0, 0.0005).unwrap();
        assert_eq!(
            parse(&["run"]).unwrap(),
            Operation::RunPulse {
                interval: Duration::from_secs(30),
                signal,
            }
        );
        assert_eq!(parse(&["test"]).unwrap(), Operation::Test(signal));
    }

    #[test]
    fn explicit_values_override_defaults_without_changing_other_fields() {
        assert_eq!(
            parse(&["run", "--interval", "60", "--volume", "0"]).unwrap(),
            Operation::RunPulse {
                interval: Duration::from_secs(60),
                signal: PulseParameters::new(300, 100.0, 0.0).unwrap(),
            }
        );
        assert_eq!(
            parse(&[
                "test",
                "--duration",
                "5000",
                "--frequency",
                "20",
                "--volume",
                "0.05"
            ])
            .unwrap(),
            Operation::Test(PulseParameters::new(5000, 20.0, 0.05).unwrap())
        );
    }

    #[test]
    fn interval_bounds_and_duration_relationship_are_strict() {
        for interval in ["1", "3600"] {
            assert!(parse(&["run", "--interval", interval]).is_ok());
        }
        for interval in ["0", "3601", "-1", "1.5", "NaN", "18446744073709551616"] {
            assert!(parse(&["run", "--interval", interval]).is_err());
        }
        assert!(parse(&["run", "--interval", "1", "--duration", "999"]).is_ok());
        for duration in ["1000", "1001"] {
            let error = parse(&["run", "--interval", "1", "--duration", duration]).unwrap_err();
            assert!(error.to_string().contains("shorter than --interval"));
        }
    }

    #[test]
    fn signal_validation_applies_to_both_commands() {
        for command in ["run", "test"] {
            for (flag, values) in [
                ("--duration", &["19", "5001", "-1", "20.5", "NaN"][..]),
                ("--frequency", &["19", "1001", "NaN", "inf", "-inf"][..]),
                ("--volume", &["-0.001", "0.051", "NaN", "inf", "-inf"][..]),
            ] {
                for value in values {
                    let error = parse(&[command, flag, value]).unwrap_err();
                    assert!(error.to_string().contains(flag), "{error}");
                }
            }
        }
    }

    #[test]
    fn silence_rejects_even_explicit_defaults_and_zero_volume() {
        assert_eq!(
            parse(&["run", "--mode", "silence"]).unwrap(),
            Operation::RunSilence
        );
        for (flag, value) in [
            ("--interval", "30"),
            ("--duration", "300"),
            ("--frequency", "100"),
            ("--volume", "0.0005"),
            ("--volume", "0"),
        ] {
            let error = parse(&["run", "--mode", "silence", flag, value]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
            assert!(error.to_string().contains(flag));
        }
    }

    #[test]
    fn test_has_no_mode_or_interval_and_unknown_input_is_rejected() {
        for args in [
            vec!["test", "--mode", "silence"],
            vec!["test", "--interval", "30"],
            vec!["run", "--mode", "unknown"],
            vec!["run", "--volum", "0.05"],
            vec!["run", "--volume"],
            vec!["run", "--volume", "0.001", "--volume", "0.05"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn verbose_is_global_and_does_not_affect_validation() {
        for args in [
            vec!["speaker-keepalive", "--verbose", "run"],
            vec!["speaker-keepalive", "run", "--verbose"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.verbose);
            assert!(cli.into_operation().is_ok());
        }
        assert!(parse(&["run", "--mode", "silence", "--verbose"]).is_ok());
    }
}

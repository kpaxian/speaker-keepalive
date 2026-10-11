mod audio;
mod backend;
mod cli;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Operation};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    let operation = cli.into_operation().unwrap_or_else(|error| error.exit());

    let result = match operation {
        Operation::Test(signal) => backend::play_once(&signal.generate(), verbose),
        Operation::RunPulse { interval, signal } => backend::run_pulses(&signal, interval, verbose),
        Operation::RunSilence => backend::run_silence(verbose),
        Operation::RunTone(signal) => backend::run_tone(signal, verbose),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            if verbose {
                eprintln!("interrupted; playback child stopped and reaped");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

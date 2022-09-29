use clap::{ArgAction, Parser};
use log::LevelFilter;
use std::path::PathBuf;

/// CLI configuration options.
#[derive(Debug, Parser)]
pub(crate) struct CliOptions {
    /// Verbosity level (higher is more verbose).
    #[clap(short = 'v', action = ArgAction::Count)]
    verbosity: u8,

    /// Path to configuration file.
    #[clap(short = 'c')]
    pub config_path: PathBuf,
}

impl CliOptions {
    /// Returns the log-level set via command-line flags.
    pub(crate) fn loglevel(&self) -> LevelFilter {
        match self.verbosity {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            _ => LevelFilter::Trace,
        }
    }
}

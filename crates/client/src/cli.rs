use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// A fictional versioning CLI
#[derive(Debug, Parser)]
#[clap(about = "The PortalBox CLI", long_about = None)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Commands,
    /// Custom config file location
    #[clap(long, global = true)]
    pub config_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start the portalbox client
    Start,
    /// Reset data
    Reset(Reset),
}

#[derive(Debug, Args)]
pub struct Reset {
    #[clap(subcommand)]
    pub command: ResetCommands,
}

#[derive(Debug, Subcommand)]
pub enum ResetCommands {
    /// Delete saved credentials
    Credentials,
    /// Uninstall all apps
    Apps,
    /// Clear apps data
    AppsData,
    /// Reset everything
    All,
}
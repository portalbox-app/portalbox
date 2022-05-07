use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[clap(name = "portalbox")]
#[clap(about = "The PortalBox Client", long_about = None)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Option<Commands>,
    /// Custom config file location
    #[clap(long, global = true)]
    pub config_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start the portalbox client
    Start,
    /// Create a TLS connection to host and redir to stdin/out
    Tunnel { host: String },
    /// Show current config
    Config,
    /// Reset data
    Reset(Reset),
    /// Show current version
    Version,
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

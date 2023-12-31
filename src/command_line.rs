//! The command-line arguments for `openrpc-gen`.

use std::path::PathBuf;

/// A CLI tool to parse OpenRPC documents and generate Rust types from them.
#[derive(Debug, Clone, clap::Parser)]
pub struct CommandLineArgs {
    /// The path to the configuration file to use.
    #[clap(short, long)]
    pub config: PathBuf,
    /// The OpenRPC document to be parsed.
    #[clap(short, long)]
    pub document: PathBuf,
    /// The path to the output file.
    #[clap(short, long)]
    pub output: PathBuf,
}

/// Loads an instance of [`CommandLineArgs`] from the environment.
///
/// If an error occurs or if the user requests help, the program will exit, eventually leaking
/// memory if some destructors are not run.
pub fn from_env() -> CommandLineArgs {
    clap::Parser::parse()
}

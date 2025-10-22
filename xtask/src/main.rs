use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod build;
mod command_context;
#[cfg(feature = "full")]
mod lfs;
#[cfg(feature = "full")]
mod ptr;
#[cfg(feature = "full")]
mod sha256;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Task,
    #[clap(flatten)]
    global_args: command_context::GlobalArgs,
}

#[derive(Subcommand)]
enum Task {
    Build(build::BuildCommand),
    Install(build::InstallCommand),
    #[cfg(feature = "full")]
    #[clap(subcommand)]
    Ptr(ptr::PtrCommand),
}

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").context("`CARGO_MANIFEST_DIR` is not set")?,
    );
    let Args {
        command,
        global_args,
    } = Args::parse();
    let ctx = command_context::CommandContext::new(manifest_dir, global_args);

    match command {
        Task::Build(build) => build::build_library(&ctx, &build),
        Task::Install(install) => build::install_library(&ctx, install),
        #[cfg(feature = "full")]
        Task::Ptr(ptr) => ptr.run(&ctx),
    }
}

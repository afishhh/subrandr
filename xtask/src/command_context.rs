use std::{
    cell::OnceCell,
    io::IsTerminal as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::CargoMetadata;
use clap::Parser;

pub mod cargo_metadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet = -1,
    Normal = 0,
    Verbose = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TermColor {
    Green,
}

impl TermColor {
    fn bold_ansi_str(self) -> &'static str {
        match self {
            TermColor::Green => "\x1b[32;1m",
        }
    }
}

#[derive(Parser)]
pub struct GlobalArgs {
    #[clap(global = true, short = 'q', long = "quiet")]
    quiet: bool,
    #[clap(global = true, short = 'v', long = "verbose", conflicts_with = "quiet")]
    verbose: bool,
}

pub struct CommandContext {
    manifest_dir: PathBuf,
    verbosity: Verbosity,
    cargo_metadata: OnceCell<CargoMetadata>,
}

impl CommandContext {
    pub fn new(manifest_dir: PathBuf, args: GlobalArgs) -> Self {
        Self {
            manifest_dir,
            verbosity: {
                if args.quiet {
                    Verbosity::Quiet
                } else if args.verbose {
                    Verbosity::Verbose
                } else {
                    Verbosity::Normal
                }
            },
            cargo_metadata: OnceCell::new(),
        }
    }

    pub fn manifest_dir(&self) -> &Path {
        &self.manifest_dir
    }

    pub fn project_dir(&self) -> &Path {
        self.manifest_dir.parent().unwrap()
    }

    pub fn verbosity(&self) -> Verbosity {
        self.verbosity
    }

    pub fn cargo_metadata(&self) -> Result<&CargoMetadata> {
        // TODO: top 10 missing std features: `once_cell_try`
        if let Some(meta) = self.cargo_metadata.get() {
            return Ok(meta);
        }

        let output = std::process::Command::new(env!("CARGO"))
            .arg("metadata")
            .arg("--locked")
            .arg("--offline")
            .arg("--no-deps")
            .arg("--format-version=1")
            .output()
            .context("Failed to run `cargo metadata`")?;
        if !output.status.success() {
            bail!("`cargo metadata` failed: {}", output.status)
        }

        let meta = serde_json::from_slice(&output.stdout)
            .context("Failed to parse `cargo metadata` output")?;
        Ok(self.cargo_metadata.get_or_init(|| meta))
    }
}

#[doc(hidden)]
pub fn _statusln(title: &str, color: TermColor, args: &std::fmt::Arguments) {
    assert!(title.len() <= 12);

    if std::io::stderr().is_terminal() {
        eprint!("{}{title: >12}\x1b[0m ", color.bold_ansi_str());
    } else {
        eprint!("{title: >12} ");
    }
    eprintln!("{args}");
}

macro_rules! statusln {
    ($ctx: expr, $title: literal, $($fmt: tt)*) => {{
        statusln!($ctx, Normal, Green, $title, $($fmt)*)
    }};
    ($ctx: expr, $verbosity: ident, $color: ident, $title: literal, $($fmt: tt)*) => {{
        const {
            assert!($title.len() <= 12);
        }
        let ctx: &$crate::command_context::CommandContext = &$ctx;
        if ctx.verbosity() >= $crate::command_context::Verbosity::$verbosity {
            crate::command_context::_statusln(
                $title,
                $crate::command_context::TermColor::$color,
                &format_args!($($fmt)*)
            )
        }
    }};
}

pub(crate) use statusln;

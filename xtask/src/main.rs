use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    Install(InstallCommand),
}

#[derive(Parser)]
struct InstallCommand {
    #[clap(short = 't', long = "target", default_value_t = env!("TARGET").to_owned())]
    target: String,
    #[clap(short = 'p', long = "prefix")]
    prefix: Option<PathBuf>,
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in src.read_dir()? {
        let entry = entry?;
        let entry_dst = dst.join(entry.file_name());
        let entry_type = entry.file_type()?;
        if entry_type.is_dir() {
            copy_dir_all(&entry.path(), &entry_dst)?;
        } else {
            std::fs::copy(entry.path(), entry_dst)?;
        }
    }

    Ok(())
}

fn make_pkgconfig_file(prefix: &Path, version: &str, target: &str) -> String {
    let prefix_str = prefix.to_str().unwrap().trim_end_matches('/');
    let extra_link_flags = if target.contains("-windows-") {
        "-lWs2_32 -lUserenv"
    } else {
        ""
    };
    let extra_requires = if target.contains("-windows-") {
        ""
    } else {
        ", fontconfig >= 2"
    };

    format!(
        r#"prefix={prefix_str}
libdir={prefix_str}/lib
includedir={prefix_str}/include

Name: subrandr
Description: A subtitle rendering library
Version: {version}
Requires.private: freetype2 >= 26, harfbuzz >= 10{extra_requires}
Cflags: -I${{includedir}}
Libs: -L${{libdir}} -lsubrandr
Libs.private: {extra_link_flags}
"#
    )
}

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").context("`CARGO_MANIFEST_DIR` is not set")?,
    );

    let project_dir = manifest_dir.parent().unwrap();

    match Args::parse().command {
        Task::Install(install) => {
            let prefix = install
                .prefix
                .or_else(|| std::env::var_os("PREFIX").map(PathBuf::from))
                .context(
                    "Either of `--prefix` or the `PREFIX` environment variable is required.",
                )?;

            let status = Command::new(env!("CARGO"))
                .arg("build")
                .arg("--target")
                .arg(&install.target)
                .arg("--release")
                .arg("-p")
                .arg("subrandr")
                .status()
                .context("Failed to run `cargo build`")?;
            if !status.success() {
                bail!("`cargo build` failed: {}", status)
            }

            let output = Command::new(env!("CARGO"))
                .arg("metadata")
                .arg("--no-deps")
                .output()
                .context("Failed to run `cargo metadata`")?;

            if !output.status.success() {
                bail!("`cargo metadata` failed: {}", status)
            }

            #[derive(Debug, Deserialize)]
            struct Metadata {
                packages: Vec<PackageMetadata>,
            }

            #[derive(Debug, Deserialize)]
            struct PackageMetadata {
                name: Box<str>,
                version: Box<str>,
            }

            let metadata = serde_json::from_slice::<Metadata>(&output.stdout)
                .context("Failed to deserialize `cargo metadata` output")?
                .packages
                .into_iter()
                .find(|package| &*package.name == "subrandr")
                .context("Failed to find metadata for package `subrandr`")?;

            (|| -> Result<()> {
                std::fs::create_dir_all(prefix.join("lib").join("pkgconfig"))?;
                std::fs::create_dir_all(prefix.join("include").join("subrandr"))?;
                Ok(())
            })()
            .context("Failed to create directory structure")?;

            copy_dir_all(
                &project_dir.join("include"),
                &prefix.join("include").join("subrandr"),
            )
            .context("Failed to copy headers")?;

            let target_dir = project_dir
                .join("target")
                .join(&install.target)
                .join("release");

            std::fs::copy(
                target_dir.join("libsubrandr.a"),
                prefix.join("lib").join("libsubrandr.a"),
            )
            .context("Failed to copy `libsubrandr.a`")?;

            let shared_name = if install.target.contains("-windows-") {
                "subrandr.dll"
            } else {
                "libsubrandr.so"
            };

            std::fs::copy(
                target_dir.join(shared_name),
                prefix.join("lib").join(shared_name),
            )
            .with_context(|| format!("Failed to copy `{shared_name}`"))?;

            std::fs::write(
                prefix.join("lib").join("pkgconfig").join("subrandr.pc"),
                make_pkgconfig_file(&prefix, &metadata.version, &install.target),
            )
            .context("Failed to write pkgconfig file")?;
        }
    }

    Ok(())
}

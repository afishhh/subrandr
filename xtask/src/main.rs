use std::{
    fmt::Display,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
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
    #[clap(short = 't', long = "target", default_value = env!("TARGET"))]
    target: Triple,
    #[clap(short = 'p', long = "prefix")]
    prefix: Option<PathBuf>,
    #[clap(long = "destdir")]
    destdir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct Triple {
    arch: Box<str>,
    vendor: Box<str>,
    os: Box<str>,
    env: Option<Box<str>>,
}

impl Triple {
    fn is_windows(&self) -> bool {
        &*self.os == "windows"
    }

    fn is_unix(&self) -> bool {
        // Censored version of `rustc -Z unstable-options --print all-target-specs-json | jq -r '.[] | select(.["target-family"] | index("unix") != null) | .os' | sort | uniq`
        matches!(
            &*self.os,
            "android"
                | "cygwin"
                | "emscripten"
                | "freebsd"
                | "ios"
                | "linux"
                | "macos"
                | "netbsd"
                | "openbsd"
                | "tvos"
                | "visionos"
                | "vita"
        )
    }
}

impl FromStr for Triple {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let mut it = text.split('-');
        let result = Triple {
            arch: it.next().unwrap().into(),
            vendor: it.next().context("Target triple missing vendor")?.into(),
            os: it.next().context("Target triple missing os")?.into(),
            env: it.next().map(Into::into),
        };

        if it.next().is_some() {
            bail!("Target triple contains too many components")
        }

        Ok(result)
    }
}

impl Display for Triple {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.arch, self.vendor, self.os)?;

        if let Some(env) = self.env.as_ref() {
            write!(f, "-{env}")?;
        }

        Ok(())
    }
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

fn copy_file(src: &Path, dst: &Path, file: &str) -> anyhow::Result<()> {
    std::fs::copy(src.join(file), dst.join(file))
        .with_context(|| format!("Failed to copy `{file}`"))
        .map(|_| ())
}

fn make_pkgconfig_file(prefix: &Path, version: &str, target: &Triple) -> String {
    let prefix_str = prefix.to_str().unwrap().trim_end_matches('/');
    let extra_link_flags = if target.is_windows() {
        "-lWs2_32 -lUserenv"
    } else {
        ""
    };
    let extra_requires = if target.is_unix() {
        ", fontconfig >= 2"
    } else {
        ""
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

fn write_implib(arch: &str, def_content: &str, dllname: &str, output_path: &Path) -> Result<()> {
    let machine = match arch {
        "x86_64" => implib::MachineType::AMD64,
        "i686" => implib::MachineType::I386,
        "aarch64" => implib::MachineType::ARM64,
        _ => panic!("Don't know how to generate implib for arch {arch:?}"),
    };

    let mut def = implib::def::ModuleDef::parse(def_content, machine)
        .context("Failed to parse module def")?;
    def.import_name = dllname.to_owned();
    implib::ImportLibrary::from_def(def, machine, implib::Flavor::Gnu)
        .write_to(
            &mut std::fs::File::create(output_path)
                .context("Failed to open import library output file")?,
        )
        .context("Failed to create import library")
}

#[derive(Debug, Deserialize)]
struct Manifest {
    package: Package,
}

#[derive(Debug, Deserialize)]
struct Package {
    version: Box<str>,
    metadata: PackageMetadata,
}

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    capi: CApiMetadata,
}

#[derive(Debug, Deserialize)]
struct CApiMetadata {
    abiver: Box<str>,
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
            let destdir = install
                .destdir
                .or_else(|| std::env::var_os("DESTDIR").map(PathBuf::from))
                .unwrap_or_else(|| prefix.clone());

            let status = Command::new(env!("CARGO"))
                .arg("build")
                .arg("--target")
                .arg(install.target.to_string())
                .arg("--release")
                .arg("-p")
                .arg("subrandr")
                .status()
                .context("Failed to run `cargo build`")?;
            if !status.success() {
                bail!("`cargo build` failed: {}", status)
            }

            let manifest: Manifest = toml::from_str(
                &std::fs::read_to_string(project_dir.join("Cargo.toml"))
                    .context("Failed to read Cargo.toml")?,
            )
            .context("Failed to parse Cargo.toml")?;

            let version = manifest.package.version;
            let abiver = manifest.package.metadata.capi.abiver;

            (|| -> Result<()> {
                std::fs::create_dir_all(destdir.join("lib").join("pkgconfig"))?;
                std::fs::create_dir_all(destdir.join("include").join("subrandr"))?;
                if install.target.is_windows() {
                    std::fs::create_dir_all(destdir.join("bin"))?;
                }
                Ok(())
            })()
            .context("Failed to create directory structure")?;

            copy_dir_all(
                &project_dir.join("include"),
                &destdir.join("include").join("subrandr"),
            )
            .context("Failed to copy headers")?;

            let target_dir = project_dir
                .join("target")
                .join(install.target.to_string())
                .join("release");

            let libdir = destdir.join("lib");

            copy_file(&target_dir, &libdir, "libsubrandr.a")?;

            let (shared_in, shared_dir, shared_out) = if install.target.is_windows() {
                ("subrandr.dll", "bin", format!("subrandr-{abiver}.dll"))
            } else {
                ("libsubrandr.so", "lib", format!("libsubrandr.so.{abiver}"))
            };

            std::fs::copy(
                target_dir.join(shared_in),
                destdir.join(shared_dir).join(&shared_out),
            )
            .with_context(|| format!("Failed to copy `{shared_in}`"))?;

            #[cfg(unix)]
            if install.target.is_unix() {
                let link = libdir.join("libsubrandr.so");
                match std::fs::remove_file(&link) {
                    Ok(()) => (),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
                    Err(err) => return Err(err).context("Failed to remove `libsubrandr.so`"),
                };
                std::os::unix::fs::symlink(&shared_out, link)
                    .context("Failed to symlink `libsubrandr.so`")?;
            }

            if install.target.is_windows() {
                write_implib(
                    &install.target.arch,
                    &std::fs::read_to_string(target_dir.join("subrandr.def"))
                        .context("Failed to read module definition file")?,
                    &shared_out,
                    &libdir.join("libsubrandr.dll.a"),
                )?;
            }

            std::fs::write(
                libdir.join("pkgconfig").join("subrandr.pc"),
                make_pkgconfig_file(&prefix, &version, &install.target),
            )
            .context("Failed to write pkgconfig file")?;
        }
    }

    Ok(())
}

use std::{
    ffi::OsString,
    fmt::Display,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;

use crate::command_context::{CommandContext, Verbosity, statusln};

#[derive(Parser)]
pub struct BuildCommand {
    #[clap(short = 't', long = "target", default_value = env!("TARGET"))]
    target: Triple,
    #[clap(
        long = "shared-library",
        action = clap::ArgAction::Set,
        default_value = "true"
    )]
    shared_library: bool,
    #[clap(
        long = "static-library",
        action = clap::ArgAction::Set,
        default_value = "false"
    )]
    static_library: bool,
    /// Arguments passed through to `cargo rustc`.
    cargo_rustc_args: Vec<OsString>,
}

#[derive(Parser)]
pub struct InstallCommand {
    #[clap(flatten)]
    build: BuildCommand,
    #[clap(short = 'p', long = "prefix")]
    prefix: Option<PathBuf>,
    #[clap(long = "destdir")]
    destdir: Option<PathBuf>,
    #[clap(long = "bindir", default_value = "bin")]
    bindir: PathBuf,
    #[clap(long = "libdir", default_value = "lib")]
    libdir: PathBuf,
    #[clap(long = "includedir", default_value = "include")]
    includedir: PathBuf,
    #[clap(long = "pkgconfigdir")]
    pkgconfigdir: Option<PathBuf>,
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

    fn is_macos(&self) -> bool {
        matches!(&*self.os, "darwin")
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

fn get_required_system_static_libs(target: &Triple) -> Result<String> {
    let mut process = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("--target")
        .arg(target.to_string())
        .arg("--crate-type=staticlib")
        .arg("--print=native-static-libs")
        .arg("-")
        .arg("-o")
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn rustc")?;

    let mut stderr = BufReader::new(process.stderr.as_mut().unwrap());
    let mut line = String::new();
    while stderr.read_line(&mut line)? != 0 {
        const NEEDLE: &str = "native-static-libs: ";
        if let Some(offset) = line.find(NEEDLE) {
            drop(stderr);
            process.wait()?;
            return Ok(line[offset + NEEDLE.len()..].trim().to_owned());
        }
        line.clear();
    }

    process.wait()?;
    bail!("Failed to extract required static libraries from rustc output")
}

fn make_pkgconfig_file(
    prefix: &Path,
    version: &str,
    target: &Triple,
    libdir: &Path,
    includedir: &Path,
    static_library: bool,
) -> Result<String> {
    let prefix_str = prefix.to_str().unwrap().trim_end_matches('/');
    let required_static_libs = if static_library {
        get_required_system_static_libs(target)
            .context("Failed to query rustc for required static libraries")?
    } else {
        String::new()
    };
    let extra_requires = if target.is_unix() {
        ", fontconfig >= 2"
    } else {
        ""
    };

    let libdir_str = [
        prefix_str,
        "/",
        libdir.to_str().unwrap().trim_end_matches('/'),
    ]
    .concat();
    let includedir_str = [
        prefix_str,
        "/",
        includedir.to_str().unwrap().trim_end_matches('/'),
    ]
    .concat();

    Ok(format!(
        r#"prefix={prefix_str}
libdir={libdir_str}
includedir={includedir_str}

Name: subrandr
Description: A subtitle rendering library
Version: {version}
Requires.private: freetype2 >= 26, harfbuzz >= 10{extra_requires}
Cflags: -I${{includedir}}
Libs: -L${{libdir}} -lsubrandr
Libs.private: {required_static_libs}
"#
    ))
}

fn target_has_broken_implib(target: &Triple) -> bool {
    target.is_windows()
        && target.env.as_deref().is_some_and(|env| env != "msvc")
        && matches!(&*target.arch, "i686")
}

fn write_implib(
    target: &Triple,
    def_content: &str,
    dllname: &str,
    output_path: &Path,
) -> Result<()> {
    let flavor = match target.env.as_deref() {
        Some("msvc") => implib::Flavor::Msvc,
        _ => implib::Flavor::Gnu,
    };

    let machine = match &*target.arch {
        "x86_64" => implib::MachineType::AMD64,
        "i686" => implib::MachineType::I386,
        "aarch64" => implib::MachineType::ARM64,
        _ => panic!(
            "Don't know how to generate implib for arch {:?}",
            target.arch
        ),
    };

    let mut def = implib::def::ModuleDef::parse(def_content, machine)
        .context("Failed to parse module def")?;
    def.import_name = dllname.to_owned();
    implib::ImportLibrary::from_def(def, machine, flavor)
        .write_to(
            &mut std::fs::File::create(output_path)
                .context("Failed to open import library output file")?,
        )
        .context("Failed to create import library")
}

#[derive(Debug, Deserialize)]
struct CApiMetadata {
    abiver: Box<str>,
}

pub fn build_library(ctx: &CommandContext, build: &BuildCommand) -> Result<()> {
    if !build.shared_library && !build.static_library {
        return Ok(());
    }

    let status = Command::new(env!("CARGO"))
        .arg("rustc")
        .arg("--manifest-path")
        .arg(ctx.manifest_dir().join("Cargo.toml"))
        .arg("--target")
        .arg(build.target.to_string())
        .arg("--crate-type")
        .arg({
            let mut types = String::new();
            if build.shared_library {
                types.push_str("cdylib")
            }
            if build.static_library {
                if !types.is_empty() {
                    types.push(',');
                }
                types.push_str("staticlib");
            }
            types
        })
        .arg("--release")
        .arg("-p")
        .arg("subrandr")
        .args(if ctx.verbosity() <= Verbosity::Quiet {
            &["--quiet"][..]
        } else {
            &[][..]
        })
        .args(&build.cargo_rustc_args)
        .status()
        .context("Failed to run `cargo rustc`")?;

    if !status.success() {
        bail!("`cargo rustc` failed: {}", status)
    }

    Ok(())
}

pub fn install_library(ctx: &CommandContext, install: InstallCommand) -> Result<()> {
    let prefix = install
        .prefix
        .or_else(|| std::env::var_os("PREFIX").map(PathBuf::from))
        .context("Either of `--prefix` or the `PREFIX` environment variable is required.")?;
    let destdir = install
        .destdir
        .or_else(|| std::env::var_os("DESTDIR").map(PathBuf::from))
        .unwrap_or_else(|| prefix.clone());

    if target_has_broken_implib(&install.build.target) {
        eprintln!(
            "\x1b[1;31mERROR\x1b[0m: Building for {} is currently known to be broken!",
            install.build.target
        );
        eprintln!(
            "\x1b[1;31mERROR\x1b[0m: See issue #31 at https://github.com/afishhh/subrandr/issues/31"
        );
        panic!("Build for broken platform aborted");
    }

    build_library(ctx, &install.build)?;

    let package = ctx
        .cargo_metadata()?
        .packages
        .iter()
        .find(|&p| &*p.name == "subrandr")
        .context("Package `subrandr` is missing from cargo metadata")?;

    let version = &package.version;
    let abiver = package
        .metadata
        .try_parse_key::<CApiMetadata>("capi")
        .context("Failed to parse `capi` metadata table")?
        .abiver;

    let libdir = destdir.join(&install.libdir);
    let pkgconfigdir = if let Some(pkgconfigdir) = install.pkgconfigdir {
        destdir.join(pkgconfigdir)
    } else {
        libdir.join("pkgconfig")
    };

    (|| -> Result<()> {
        if install.build.target.is_windows() {
            std::fs::create_dir_all(destdir.join(&install.bindir))?;
        }
        std::fs::create_dir_all(&libdir)?;
        std::fs::create_dir_all(destdir.join(&install.includedir).join("subrandr"))?;
        std::fs::create_dir_all(&pkgconfigdir)?;
        Ok(())
    })()
    .context("Failed to create directory structure")?;

    let include_dir = destdir.join(&install.includedir);
    statusln!(ctx, "Installing", "headers to `{}`", include_dir.display());
    copy_dir_all(
        &ctx.project_dir().join("include"),
        &include_dir.join("subrandr"),
    )
    .context("Failed to copy headers")?;

    let target_dir = ctx
        .project_dir()
        .join("target")
        .join(install.build.target.to_string())
        .join("release");

    if install.build.static_library {
        statusln!(ctx, "Installing", "libsubrandr.a to `{}`", libdir.display());
        copy_file(&target_dir, &libdir, "libsubrandr.a")?;
    }

    let (shared_in, shared_dir, shared_out) = if install.build.target.is_windows() {
        (
            "subrandr.dll",
            &install.bindir,
            format!("subrandr-{abiver}.dll"),
        )
    } else if install.build.target.is_macos() {
        (
            "libsubrandr.dylib",
            &install.libdir,
            "libsubrandr.dylib".to_owned(),
        )
    } else {
        (
            "libsubrandr.so",
            &install.libdir,
            format!("libsubrandr.so.{abiver}"),
        )
    };

    if install.build.shared_library {
        let full_shared_dir = destdir.join(shared_dir);
        statusln!(
            ctx,
            "Installing",
            "{shared_out} to `{}`",
            full_shared_dir.display()
        );
        std::fs::copy(
            target_dir.join(shared_in),
            full_shared_dir.join(&shared_out),
        )
        .with_context(|| format!("Failed to copy `{shared_in}`"))?;

        #[cfg(unix)]
        if install.build.target.is_unix() && !install.build.target.is_macos() {
            let link = libdir.join("libsubrandr.so");
            match std::fs::remove_file(&link) {
                Ok(()) => (),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
                Err(err) => return Err(err).context("Failed to remove `libsubrandr.so`"),
            };
            std::os::unix::fs::symlink(&shared_out, link)
                .context("Failed to symlink `libsubrandr.so`")?;
        }

        if install.build.target.is_windows() {
            statusln!(ctx, "Installing", "implib to `{}`", libdir.display());
            write_implib(
                &install.build.target,
                &std::fs::read_to_string(target_dir.join("subrandr.def"))
                    .context("Failed to read module definition file")?,
                &shared_out,
                &libdir.join("libsubrandr.dll.a"),
            )?;
        }
    }

    statusln!(
        ctx,
        "Installing",
        "subrandr.pc to `{}`",
        pkgconfigdir.display()
    );

    std::fs::write(
        pkgconfigdir.join("subrandr.pc"),
        make_pkgconfig_file(
            &prefix,
            version,
            &install.build.target,
            &install.libdir,
            &install.includedir,
            install.build.static_library,
        )?,
    )
    .context("Failed to write pkgconfig file")
}

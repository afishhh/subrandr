use std::{
    ffi::OsString,
    fmt::Display,
    io::{BufRead, BufReader, IsTerminal},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Task,
    #[clap(short = 'q', long = "quiet", global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Task {
    Build(BuildCommand),
    Install(InstallCommand),
}

#[derive(Parser)]
struct BuildCommand {
    #[clap(short = 't', long = "target", default_value = env!("TARGET"))]
    target: Triple,
    /// Arguments passed through to `cargo build`.
    cargo_args: Vec<OsString>,
}

#[derive(Parser)]
struct InstallCommand {
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
struct Manifest {
    package: Package,
    workspace: WorkspaceMetadata,
}

#[derive(Debug, Deserialize)]
struct Package {
    metadata: PackageMetadata,
}

#[derive(Debug, Deserialize)]
struct WorkspaceMetadata {
    package: WorkspacePackageMetadata,
}

#[derive(Debug, Deserialize)]
struct WorkspacePackageMetadata {
    version: Box<str>,
}

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    capi: CApiMetadata,
}

#[derive(Debug, Deserialize)]
struct CApiMetadata {
    abiver: Box<str>,
}

fn build_library(manifest_dir: &Path, build: &BuildCommand, quiet: bool) -> Result<()> {
    let status = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest_dir.join("Cargo.toml"))
        .arg("--target")
        .arg(build.target.to_string())
        .arg("--release")
        .arg("-p")
        .arg("subrandr")
        .args(if quiet { &["--quiet"][..] } else { &[][..] })
        .args(&build.cargo_args)
        .status()
        .context("Failed to run `cargo build`")?;

    if !status.success() {
        bail!("`cargo build` failed: {}", status)
    }

    Ok(())
}

fn print_cargo_style_status(title: &str, args: &std::fmt::Arguments) {
    assert!(title.len() <= 12);

    if std::io::stderr().is_terminal() {
        eprint!("\x1b[32;1m{title: >12}\x1b[0m ");
    } else {
        eprint!("{title: >12} ");
    }
    eprintln!("{args}");
}

macro_rules! statusln {
    ($title: literal, $($fmt: tt)*) => {
        print_cargo_style_status($title, &format_args!($($fmt)*))
    };
}

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").context("`CARGO_MANIFEST_DIR` is not set")?,
    );

    let project_dir = manifest_dir.parent().unwrap();

    let args = Args::parse();
    match args.command {
        Task::Build(build) => {
            build_library(&manifest_dir, &build, args.quiet)?;
        }
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

            build_library(&manifest_dir, &install.build, args.quiet)?;

            let manifest: Manifest = toml::from_str(
                &std::fs::read_to_string(project_dir.join("Cargo.toml"))
                    .context("Failed to read Cargo.toml")?,
            )
            .context("Failed to parse Cargo.toml")?;

            let version = manifest.workspace.package.version;
            let abiver = manifest.package.metadata.capi.abiver;

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
            if !args.quiet {
                statusln!("Installing", "headers to `{}`", include_dir.display());
            }
            copy_dir_all(&project_dir.join("include"), &include_dir.join("subrandr"))
                .context("Failed to copy headers")?;

            let target_dir = project_dir
                .join("target")
                .join(install.build.target.to_string())
                .join("release");

            if install.static_library {
                if !args.quiet {
                    statusln!("Installing", "libsubrandr.a to `{}`", libdir.display());
                }
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

            if install.shared_library {
                let full_shared_dir = destdir.join(shared_dir);
                if !args.quiet {
                    statusln!(
                        "Installing",
                        "{shared_out} to `{}`",
                        full_shared_dir.display()
                    );
                }
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
                    if !args.quiet {
                        statusln!("Installing", "implib to `{}`", libdir.display());
                    }
                    write_implib(
                        &install.build.target,
                        &std::fs::read_to_string(target_dir.join("subrandr.def"))
                            .context("Failed to read module definition file")?,
                        &shared_out,
                        &libdir.join("libsubrandr.dll.a"),
                    )?;
                }
            }

            if !args.quiet {
                statusln!("Installing", "subrandr.pc to `{}`", pkgconfigdir.display());
            }
            std::fs::write(
                pkgconfigdir.join("subrandr.pc"),
                make_pkgconfig_file(
                    &prefix,
                    &version,
                    &install.build.target,
                    &install.libdir,
                    &install.includedir,
                    install.static_library,
                )?,
            )
            .context("Failed to write pkgconfig file")?;
        }
    }

    Ok(())
}

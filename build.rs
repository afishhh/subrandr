use std::{collections::HashSet, path::PathBuf, process::Command};

struct Target {
    unix: bool,
    os: Box<str>,
    env: Box<str>,
}

impl Target {
    fn read() -> Self {
        Self {
            unix: std::env::var_os("CARGO_CFG_UNIX").is_some(),
            os: std::env::var("CARGO_CFG_TARGET_OS").unwrap().into(),
            env: std::env::var("CARGO_CFG_TARGET_ENV").unwrap().into(),
        }
    }

    fn is_windows(&self) -> bool {
        matches!(&*self.os, "windows")
    }

    fn is_android(&self) -> bool {
        matches!(&*self.os, "android")
    }
}

struct Features {
    enabled_by_script: HashSet<&'static str>,
}

impl Features {
    fn new() -> Self {
        Self {
            enabled_by_script: HashSet::new(),
        }
    }

    fn enable(&mut self, name: &'static str) {
        println!("cargo::rustc-cfg=feature=\"{name}\"");
        self.enabled_by_script.insert(name);
    }

    fn is_enabled(&self, name: &'static str) -> bool {
        self.enabled_by_script.contains(name) || {
            let var_name = format!(
                "CARGO_FEATURE_{}",
                name.to_ascii_uppercase().replace('-', "_")
            );
            std::env::var_os(var_name).is_some()
        }
    }
}

const FEATURE_FP_DIRECTWRITE: &str = "font-provider-directwrite";
const FEATURE_FP_FONTCONFIG: &str = "font-provider-fontconfig";
const FEATURE_FP_ANDROID_NDK: &str = "font-provider-android-ndk";

fn add_default_features(target: &Target, features: &mut Features) {
    if std::env::var_os("CARGO_FEATURE_PLATFORM_DEFAULTS").is_some() {
        if target.is_windows() {
            features.enable(FEATURE_FP_DIRECTWRITE);
        }

        if target.unix && !target.is_android() {
            features.enable(FEATURE_FP_FONTCONFIG);
        }

        if target.is_android() {
            features.enable(FEATURE_FP_ANDROID_NDK);
        }
    }
}

fn setup_aliases_and_link_libraries_for_features(target: &Target, features: &Features) {
    fn register_cfg(name: &str, values: &[&str]) {
        print!("cargo::rustc-check-cfg=cfg({name}, values(");
        let mut first = true;
        for value in values {
            if !first {
                print!(",")
            }
            first = false;
            print!("{value:?}")
        }
        println!("))")
    }

    fn enable_fp_alias(name: &str) {
        println!("cargo::rustc-cfg=font_provider=\"{name}\"")
    }

    register_cfg(
        "font_provider",
        &["fontconfig", "directwrite", "android-ndk"],
    );

    if target.is_windows() && features.is_enabled(FEATURE_FP_DIRECTWRITE) {
        enable_fp_alias("directwrite");
    }

    if target.unix && features.is_enabled(FEATURE_FP_FONTCONFIG) {
        enable_fp_alias("fontconfig");
        println!("cargo::rustc-link-lib=fontconfig");
    }

    if target.is_android() && features.is_enabled(FEATURE_FP_ANDROID_NDK) {
        enable_fp_alias("android-ndk");
    }
}

fn set_build_rev() {
    if let Ok(rev) = std::env::var("SUBRANDR_BUILD_REV") {
        println!("cargo:rustc-env=BUILD_REV_SUFFIX= rev {}", &rev[..7]);
        println!("cargo:rustc-env=BUILD_DIRTY=");
    } else if std::fs::exists(".git").unwrap() {
        let rev_output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .unwrap();
        let rev = String::from_utf8(rev_output.stdout).unwrap();
        let dirty_status = Command::new("git")
            .arg("diff-index")
            .arg("--quiet")
            .arg("HEAD")
            .status()
            .unwrap();
        let is_dirty = !dirty_status.success();

        println!("cargo:rustc-env=BUILD_REV_SUFFIX= rev {}", &rev[..7]);
        println!(
            "cargo:rustc-env=BUILD_DIRTY={}",
            if is_dirty { " (dirty)" } else { "" }
        );
    } else {
        println!("cargo:rustc-env=BUILD_REV_SUFFIX=");
        println!("cargo:rustc-env=BUILD_DIRTY=");
    };
}

fn setup_abi_versioning(target: &Target) {
    let abiver = {
        let manifest_content =
            std::fs::read_to_string(std::env::var_os("CARGO_MANIFEST_PATH").unwrap()).unwrap();

        let mut result = None;
        for line in manifest_content.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };

            if name.trim() == "abiver" {
                if let Some(content) = value
                    .trim()
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                {
                    result = Some(content.to_owned());
                    break;
                }
            }
        }

        result
    }
    .unwrap();

    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap();

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // cargo in its infinite wisdom refuses to provide you the target directory in build scripts
    let target_dir = PathBuf::from_iter(
        out_dir
            .components()
            .take_while(|x| x.as_os_str().to_str().is_none_or(|s| s != "build")),
    );

    macro_rules! cdylib_link_arg {
        ($fmt: literal $($rest: tt)*) => {
            println!(concat!("cargo:rustc-cdylib-link-arg=", $fmt) $($rest)*)
        };
    }

    match (&*target.os, &*target.env) {
        ("linux", _) => {
            cdylib_link_arg!("-Wl,-soname,libsubrandr.so.{}", abiver);
        }
        ("macos", _) | ("ios", _) => {
            // Searching "darwin linker man page" is the easiest way to find information about these.
            cdylib_link_arg!("-Wl,-compatibility_version,{}", abiver);
            cdylib_link_arg!("-Wl,-current_version,{}.{}.{}", major, minor, patch);
        }
        ("windows", "gnu") => {
            // The Windows approach is, as usual, the most arcane.
            // Apparently on Windows this is solved with stub "import libraries".
            // BUT there is no way we can tell the linker at this point that
            // "I want this import library to link to subrandr-1.dll instead of
            // subrandr.dll", because why would -soname work? Then it would make sense,
            // and we don't want that.
            // So instead the approach is as follows:
            // - Generate a .def file which contains all exported and imported
            //   symbols in a format that can be processed by `dlltool` to create
            //   an import library.
            // - Generate an import library at install time, with the correct ABI
            //   versioned dllname using the `implib` crate.
            //   (finding a working `dlltool` for the target seems very fragile)
            // Alternatively it would be nice if we could convince cargo to generate a
            // "subrandr-1.dll" instead so that it could ""just"" work.
            cdylib_link_arg!("-Wl,--output-def,{}/subrandr.def", target_dir.display());
        }
        _ => (),
    }
}

fn main() {
    set_build_rev();

    let target = Target::read();

    {
        let mut features = Features::new();
        add_default_features(&target, &mut features);
        setup_aliases_and_link_libraries_for_features(&target, &features);
    }

    setup_abi_versioning(&target);
}

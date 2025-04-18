use std::process::Command;
fn main() {
    if let Ok(rev) = std::env::var("SUBRANDR_BUILD_REV") {
        println!("cargo:rustc-env=BUILD_REV={}", &rev[..7]);
        println!("cargo:rustc-env=BUILD_DIRTY=");
    } else {
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

        println!("cargo:rustc-env=BUILD_REV={}", &rev[..7]);
        println!(
            "cargo:rustc-env=BUILD_DIRTY={}",
            if is_dirty { " (dirty)" } else { "" }
        );
    }
}

use std::path::{Path, PathBuf};

use sha2::Digest as _;

pub fn project_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
}

fn read_pixel_hash_from_ptr(ptr: &Path) -> Option<String> {
    let content = match std::fs::read_to_string(ptr) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => panic!("failed to read pointer file: {err}"),
    };

    for line in content.lines() {
        if let Some(hash_str) = line.trim().strip_prefix("pixels ") {
            return Some(hash_str.trim_start().into());
        }
    }

    panic!("no pixel hash in pointer file {}", ptr.display())
}

fn hex_sha256(digest: &sha2::digest::Output<sha2::Sha256>) -> Box<str> {
    let to_hex = |v: u8| if v < 10 { b'0' + v } else { b'a' - 10 + v };
    let mut output = [0; 64];

    for (idx, value) in digest.into_iter().enumerate() {
        output[idx * 2] = to_hex(value >> 4);
        output[idx * 2 + 1] = to_hex(value & 0xF);
    }

    std::str::from_utf8(&output).unwrap().into()
}

pub fn write_png_to(
    file: std::fs::File,
    rgba_pixel_bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba_pixel_bytes)?;
    writer.finish()
}

pub fn write_png(
    path: &Path,
    rgba_pixel_bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(), png::EncodingError> {
    write_png_to(
        std::fs::File::create(path)?,
        rgba_pixel_bytes,
        width,
        height,
    )
}

pub fn check_png_snapshot(
    base_path: &Path,
    display_base_path: &str,
    rgba_pixel_bytes: &[u8],
    width: u32,
    height: u32,
) {
    let ptr_path = base_path.with_extension("png.ptr");
    let expected_pixel_hash = read_pixel_hash_from_ptr(&ptr_path);

    let pixel_hash = sha2::Sha256::new()
        .chain_update(rgba_pixel_bytes)
        .finalize();
    let pixel_hash_str = hex_sha256(&pixel_hash);

    if expected_pixel_hash.as_deref() == Some(&pixel_hash_str) {
        let result_path = base_path.with_extension("png");
        match std::fs::File::create_new(result_path) {
            Ok(file) => write_png_to(file, rgba_pixel_bytes, width, height),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(err) => Err(err.into()),
        }
        .unwrap();
    } else {
        let extension = if expected_pixel_hash.is_some() {
            "new.png"
        } else {
            "png"
        };

        let new_path = base_path.with_extension(extension);
        write_png(&new_path, rgba_pixel_bytes, width, height).unwrap();

        if let Some(expected) = &expected_pixel_hash {
            eprintln!("Pixel hash mismatch!");
            eprintln!("Expected hash: {expected}");
            eprintln!("Current hash: {pixel_hash_str}");
        } else {
            eprintln!("No expected snapshot found for test");
        }

        let display_path = format!("{display_base_path}.{extension}");
        eprintln!("New snapshot written to {display_path}");

        panic!()
    }
}

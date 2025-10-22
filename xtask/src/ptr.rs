use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Seek, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use indexmap::IndexMap;
use sha2::Digest as _;

use crate::{
    command_context::{CommandContext, Verbosity, messageln, statusln},
    lfs::{self, BatchObject, guess_api_url_from_repo_url},
    sha256::HexSha256,
};

mod path;
pub use path::*;

pub struct PtrInfo {
    pub sha256: Box<HexSha256>,
    pub size: u64,
    pub extra: IndexMap<Box<str>, Box<str>>,
}

impl PtrInfo {
    pub fn read(path: &PtrPath) -> Result<Self> {
        let mut reader = BufReader::new(std::fs::File::open(path.path())?);

        let mut fields = IndexMap::<Box<str>, Box<str>>::new();
        let mut line_no = 1;
        let mut line = String::new();
        while let 1.. = reader.read_line(&mut line)? {
            let trimmed = line.strip_suffix('\n').unwrap_or(&line).trim_matches(' ');
            if trimmed.is_empty() {
                continue;
            }

            let Some((key, value)) = trimmed.split_once(' ') else {
                bail!("Line {line_no} is missing a space delimiter");
            };

            match fields.entry(key.into()) {
                indexmap::map::Entry::Vacant(vacant) => {
                    vacant.insert(value.trim_start_matches(' ').into());
                }
                indexmap::map::Entry::Occupied(_) => {
                    bail!("Key {key} appears twice")
                }
            }

            line.clear();
            line_no += 1;
        }

        Ok(Self {
            sha256: fields
                .shift_remove("file-hash")
                .ok_or_else(|| anyhow!("`file-hash` field is missing"))?
                .parse::<&HexSha256>()
                .map(|hash| Box::new(hash.clone()))
                .context("`file-hash` field has an invalid value")?,
            size: fields
                .shift_remove("file-size")
                .ok_or_else(|| anyhow!("`file-size` field is missing"))?
                .parse()
                .context("`file-size` field has an invalid value")?,
            extra: fields,
        })
    }

    pub fn write(&self, mut to: impl Write) -> std::io::Result<()> {
        writeln!(to, "file-hash {}", self.sha256)?;
        writeln!(to, "file-size {}", self.size)?;

        for (key, value) in &self.extra {
            writeln!(to, "{key} {value}")?;
        }

        Ok(())
    }
}

#[derive(Subcommand)]
pub enum PtrCommand {
    Write(WriteCommand),
    Pull(PullCommand),
    Push(PushCommand),
}

impl PtrCommand {
    pub fn run(self, ctx: &CommandContext) -> Result<()> {
        match self {
            PtrCommand::Write(write) => write.run(ctx),
            PtrCommand::Pull(pull) => pull.run(ctx),
            PtrCommand::Push(push) => push.run(ctx),
        }
    }
}

#[derive(Parser)]
pub struct WriteCommand {
    data_paths: Vec<PathBuf>,
}

impl WriteCommand {
    fn compute_extra_for(
        &self,
        path: &Path,
        file: std::fs::File,
    ) -> Result<IndexMap<Box<str>, Box<str>>> {
        let mut extra = IndexMap::new();

        if path.as_os_str().as_encoded_bytes().ends_with(b".png") {
            let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info()?;
            let info = reader.info();
            extra.insert("width".into(), info.width.to_string().into());
            extra.insert("height".into(), info.height.to_string().into());
            if info.color_type == png::ColorType::Rgba
                && info.bit_depth == png::BitDepth::Eight
                && !info.interlaced
                && !info.is_animated()
            {
                let mut pixel_hash = sha2::Sha256::new();

                while let Some(row) = reader.next_row()? {
                    pixel_hash.update(row.data());
                }

                extra.shift_insert(
                    0,
                    "pixels".into(),
                    HexSha256::from_digest(&pixel_hash.finalize())
                        .as_str()
                        .into(),
                );
            }
        }

        Ok(extra)
    }

    fn write_one(&mut self, data_path: &Path, ctx: &CommandContext) -> Result<()> {
        let ptr_path =
            PtrPathBuf::from_data_path(data_path).context("Path does not have a file name")?;

        statusln!(ctx, "Writing", "{}", ptr_path.display());
        let (sha256, mut file) = std::fs::File::open(data_path)
            .and_then(|mut file| Ok((HexSha256::from_reader(&mut file)?, file)))
            .context("Failed to hash file")?;
        ptr_path.write(PtrInfo {
            sha256: Box::new(sha256),
            size: file
                .stream_position()
                .context("Failed to determine file size")?,
            extra: {
                file.rewind()
                    .map_err(anyhow::Error::from)
                    .and_then(|_| self.compute_extra_for(data_path, file))
                    .context("Failed to calculate extra fields for file")?
            },
        })?;

        Ok(())
    }

    pub fn run(mut self, ctx: &CommandContext) -> Result<()> {
        for data_path in std::mem::take(&mut self.data_paths) {
            self.write_one(&data_path, ctx)
                .with_context(|| format!("Failed to write pointer for {}", data_path.display()))?
        }

        Ok(())
    }
}

fn collect_committed_ptr_files(root: PathBuf, result: &mut Vec<PtrPathBuf>) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("ls-files")
        .arg("-z")
        .arg(root)
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        bail!("`git ls-files` failed with exit status: {}", output.status);
    }

    for path in output.stdout.split(|&b| b == b'\0') {
        let path = Path::new(std::str::from_utf8(path).context("Path is not valid UTF-8")?);

        if let Some(ptr_path) = PtrPath::new(path)
            && path.try_exists()?
        {
            result.push(ptr_path.to_owned());
        }
    }

    Ok(())
}

#[derive(serde::Deserialize, Debug)]
struct Config {
    #[serde(rename = "lfs-remote")]
    lfs_remote_url: String,
    #[serde(rename = "lfs-api")]
    lfs_api_url: Option<String>,
}

impl Config {
    fn load(ctx: &CommandContext) -> Result<Config> {
        ctx.cargo_metadata()?
            .workspace_metadata
            .try_parse_key("ptr")
    }

    fn get_or_guess_api_url(&self) -> Result<String> {
        match &self.lfs_api_url {
            Some(url) => Ok(url.clone()),
            None => guess_api_url_from_repo_url(&self.lfs_remote_url),
        }
    }
}

#[derive(Parser)]
pub struct PullCommand {
    paths: Vec<PtrPathBuf>,
    #[clap(short = 'u', long = "update")]
    update: bool,
}

impl PullCommand {
    fn open_destination_path(
        &self,
        ctx: &CommandContext,
        path: &Path,
        read: bool,
    ) -> Result<Option<std::fs::File>> {
        match std::fs::OpenOptions::new()
            .read(read)
            .write(true)
            .create_new(!self.update)
            .create(true)
            .truncate(true)
            .open(path)
        {
            Ok(file) => Ok(Some(file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                messageln!(
                    ctx,
                    Normal,
                    Warning,
                    "{} created in race condition, skipping",
                    path.display()
                );
                Ok(None)
            }
            Err(err) => Err(err).context(format!("Failed to open {}", path.display())),
        }
    }

    pub fn run(mut self, ctx: &CommandContext) -> Result<()> {
        if self.paths.is_empty() {
            collect_committed_ptr_files(ctx.project_dir().to_owned(), &mut self.paths)
                .context("Failed to collect ptr files in repo")?;
        }

        let mut hash_to_ptr: HashMap<Box<HexSha256>, Vec<PtrPathBuf>> = HashMap::new();
        let mut objects = Vec::new();
        let mut num_mismatched = 0;

        for path in std::mem::take(&mut self.paths) {
            let ptr = path
                .read()
                .with_context(|| format!("Failed to read ptr at {}", path.display()))?;
            let data_path = path.data_path();

            match std::fs::File::open(data_path) {
                Ok(mut file) => {
                    let file_sha256 = HexSha256::from_reader(&mut file).with_context(|| {
                        format!("Failed to hash data file {}", data_path.display())
                    })?;

                    if file_sha256 == *ptr.sha256 {
                        statusln!(
                            ctx,
                            Verbose,
                            White,
                            "Skipped",
                            "{} (up to date)",
                            data_path.display()
                        );
                        continue;
                    } else if !self.update {
                        statusln!(
                            ctx,
                            Normal,
                            Yellow,
                            "Skipped",
                            "{} (hash does not match ptr)",
                            data_path.display()
                        );
                        num_mismatched += 1;
                        continue;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
                Err(err) => {
                    return Err(err)
                        .context(format!("Failed to open data file {}", data_path.display()));
                }
            }

            let entry = hash_to_ptr.entry(ptr.sha256.clone());
            if let std::collections::hash_map::Entry::Vacant(_) = entry {
                objects.push(BatchObject {
                    sha256: ptr.sha256,
                    size: ptr.size,
                });
            }
            entry.or_default().push(path);
        }

        if !objects.is_empty() {
            let client = lfs::Client::new(Config::load(ctx)?.get_or_guess_api_url()?);

            for handle in client
                .batch(objects, None, lfs::Operation::Download)
                .context("LFS batch download API request failed")?
            {
                let Some(download) = handle.actions.download else {
                    messageln!(
                        ctx,
                        Normal,
                        Warning,
                        "no download action received for {}, skipping",
                        handle.sha256.as_short_str()
                    );
                    continue;
                };

                let mut ptrs = hash_to_ptr.remove(&handle.sha256).unwrap();
                while let Some(first) = ptrs.pop() {
                    let Some(mut first_dest) =
                        self.open_destination_path(ctx, first.data_path(), true)?
                    else {
                        continue;
                    };

                    let first_data_path = first.data_path();
                    statusln!(
                        ctx,
                        "Downloading",
                        "{} to {}",
                        handle.sha256.as_short_str(),
                        first_data_path.display()
                    );

                    std::io::copy(&mut download.execute(&client)?, &mut first_dest).with_context(
                        || {
                            format!(
                                "Failed to copy data to data file at {}",
                                first.data_path().display()
                            )
                        },
                    )?;

                    for ptr in ptrs {
                        statusln!(ctx, "Copying to", "{}", ptr.data_path().display());

                        match self.open_destination_path(ctx, ptr.data_path(), false) {
                            Ok(None) => continue,
                            Ok(Some(file)) => Ok(file),
                            Err(err) => Err(err),
                        }
                        .and_then(|mut dest| {
                            first_dest.rewind()?;
                            std::io::copy(&mut first_dest, &mut dest).map_err(anyhow::Error::from)
                        })
                        .with_context(|| {
                            format!(
                                "Failed to copy {} to {}",
                                ptr.data_path().display(),
                                first_data_path.display()
                            )
                        })?;
                    }
                    break;
                }
            }
        }

        if num_mismatched > 0 {
            messageln!(
                ctx,
                Normal,
                Note,
                "pass `--update` to overwrite {} data file{1} with a mismatched hash",
                num_mismatched,
                if num_mismatched != 1 { "s" } else { "" }
            );
        }

        Ok(())
    }
}

#[derive(Parser)]
pub struct PushCommand {
    paths: Vec<PtrPathBuf>,
}

impl PushCommand {
    pub fn run(mut self, ctx: &CommandContext) -> Result<()> {
        if self.paths.is_empty() {
            collect_committed_ptr_files(ctx.project_dir().to_owned(), &mut self.paths)
                .context("Failed to collect ptr files in repo")?;
        }

        let mut hash_to_data_path: HashMap<Box<HexSha256>, PathBuf> = HashMap::new();
        let mut objects = Vec::new();
        let mut num_mismatched = 0;

        for path in self.paths {
            let data_path = path.data_path();
            let ptr = path
                .read()
                .with_context(|| format!("Failed to read ptr at {}", path.display()))?;

            let vacant_entry = match hash_to_data_path.entry(ptr.sha256.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => continue,
                std::collections::hash_map::Entry::Vacant(vacant) => vacant,
            };

            match std::fs::File::open(data_path) {
                Ok(mut file) => {
                    let file_sha256 = HexSha256::from_reader(&mut file).with_context(|| {
                        format!("Failed to hash data file {}", data_path.display())
                    })?;

                    if file_sha256 != *ptr.sha256 {
                        statusln!(
                            ctx,
                            Verbose,
                            Yellow,
                            "Skipped",
                            "{} (hash does not match ptr)",
                            data_path.display()
                        );
                        num_mismatched += 1;
                        continue;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    statusln!(
                        ctx,
                        Verbose,
                        White,
                        "Skipped",
                        "{} (does not exist)",
                        data_path.display()
                    );
                    continue;
                }
                Err(err) => {
                    return Err(err)
                        .context(format!("Failed to open data file {}", data_path.display()));
                }
            }

            objects.push(BatchObject {
                sha256: ptr.sha256.clone(),
                size: ptr.size,
            });
            vacant_entry.insert(data_path.to_path_buf());
        }

        if !objects.is_empty() {
            let config = Config::load(ctx)?;
            let client = lfs::Client::new(config.get_or_guess_api_url()?);
            let auth = lfs::Authorisation::authenticate_with_ssh(
                &config.lfs_remote_url,
                lfs::Operation::Upload,
            )
            .context("Failed to authenticate with lfs remote")?;

            for handle in client.batch(objects, Some(&auth), lfs::Operation::Upload)? {
                let data_path = hash_to_data_path.remove(&handle.sha256).unwrap();

                if let Some(upload) = handle.actions.upload {
                    statusln!(
                        ctx,
                        "Uploading",
                        "{} from {}",
                        handle.sha256.as_short_str(),
                        data_path.display()
                    );

                    upload
                        .execute(
                            &client,
                            std::fs::File::open(&data_path).with_context(|| {
                                format!("Failed to open data file {}", data_path.display())
                            })?,
                        )
                        .with_context(|| {
                            format!(
                                "Failed to upload {} from {}",
                                handle.sha256.as_short_str(),
                                data_path.display()
                            )
                        })?;

                    if let Some(verify) = handle.actions.verify {
                        verify.execute(&client).context("Failed to verify upload")?;
                    }
                } else {
                    statusln!(
                        ctx,
                        Verbose,
                        White,
                        "Skipped",
                        "{} (already present on remote)",
                        handle.sha256.as_short_str()
                    );
                }
            }
        }

        if ctx.verbosity() == Verbosity::Normal && num_mismatched > 0 {
            messageln!(
                ctx,
                Normal,
                Note,
                "pass `--verbose` to see {num_mismatched} data file{} with a mismatched hash",
                if num_mismatched != 1 { "s" } else { "" }
            );
        }

        Ok(())
    }
}

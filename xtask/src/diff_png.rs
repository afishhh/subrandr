use std::{
    ffi::OsString,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Debug, Clone)]
enum DiffType {
    #[clap(alias = "a", name = "asymmetric")]
    /// Shows total addition/deletion in the green/red color channel respectively.
    ///
    /// Asymmetric, easier to spot alpha-only and small changes.
    AsymmetricChange,
    #[clap(aliases = ["c"])]
    /// Shows difference of each color channel in the respective channel of the output.
    ///
    /// Symmetric, harder to spot alpha changes.
    Color,
}

#[derive(Parser)]
pub struct Command {
    /// Path to "old" image file.
    old: PathBuf,
    /// Path to "new" image file.
    ///
    /// If not provided, it will be synthesized from `old` by inserting ".new" before
    /// the last extension of the path (or at the end if no extension is present).
    /// For example, for an `old` path of `bear.png`, `bear.new.png` will be used as the `new` path.
    new: Option<PathBuf>,
    #[clap(short = 'o', long = "output")]
    output: Option<PathBuf>,
    #[clap(default_value = "asymmetric", short = 't', long = "type")]
    kind: DiffType,
}

fn open_bgra_png_reader(path: &Path) -> Result<png::Reader<std::io::BufReader<std::fs::File>>> {
    let read = std::io::BufReader::new(std::fs::File::open(path)?);
    let decoder = png::Decoder::new(read);
    let reader = decoder.read_info()?;

    let info = reader.info();
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || info.interlaced
        || info.is_animated()
    {
        bail!("png is not 8-bit noninterlaced rgba, or is animated")
    }

    Ok(reader)
}

impl Command {
    pub fn run(&self, _ctx: &crate::command_context::CommandContext) -> Result<()> {
        let new = match &self.new {
            Some(path) => path.clone(),
            // If a new path is not provided, insert `.new` before the last extension
            // of the old path.
            None => {
                let mut string = self.old.as_os_str().to_owned().into_encoded_bytes();
                let idx = string
                    .iter()
                    .rposition(|&c| c == b'.')
                    .unwrap_or(string.len());
                string.splice(idx..idx, *b".new");
                PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(string) })
            }
        };

        if self.output.is_none() && std::io::stdout().is_terminal() {
            bail!("No output path provided and stdout is a terminal");
        }

        let mut old_reader = open_bgra_png_reader(&self.old).context("Failed to open old image")?;
        let mut new_reader = open_bgra_png_reader(&new).context("Failed to open new image")?;

        let old_size = old_reader.info().size();
        let new_size = new_reader.info().size();
        if old_size != new_size {
            bail!("Images are not of the same size ({old_size:?} != {new_size:?})");
        }

        let output: Box<dyn Write> = if let Some(path) = &self.output {
            Box::new(std::fs::File::create(path).context("Failed to open output file")?)
        } else {
            Box::new(std::io::stdout().lock())
        };

        let mut output = png::Encoder::new(std::io::BufWriter::new(output), old_size.0, old_size.1);
        output.set_color(png::ColorType::Rgba);
        output.set_depth(png::BitDepth::Eight);
        let mut writer = output
            .write_header()
            .and_then(|w| w.into_stream_writer())
            .context("Failed to write png header")?;

        while let Some(old_row) = old_reader
            .next_row()
            .context("Failed to read row from old image")?
        {
            let new_row = new_reader
                .next_row()
                .context("Failed to read row from new image")?
                .unwrap();

            let old_data = old_row.data();
            let new_data = new_row.data();
            for (&[or, og, ob, oa], &[nr, ng, nb, na]) in
                std::iter::zip(old_data.as_chunks().0, new_data.as_chunks().0)
            {
                fn step_clamped_div(value: u16, divisor: u16) -> u8 {
                    if (1..=divisor * 10).contains(&value) {
                        10
                    } else {
                        (value / 3).min(255) as u8
                    }
                }

                writer
                    .write(&{
                        match self.kind {
                            DiffType::AsymmetricChange => {
                                let old_value = or as u16 + og as u16 + ob as u16 + oa as u16;
                                let new_value = nr as u16 + ng as u16 + nb as u16 + na as u16;
                                let max_value = old_value.max(new_value);
                                let scaled = step_clamped_div(max_value, 3);

                                let negative_change = or.saturating_sub(nr) as u16
                                    + og.saturating_sub(ng) as u16
                                    + ob.saturating_sub(nb) as u16
                                    + oa.saturating_sub(na) as u16;
                                let positive_change = nr.saturating_sub(or) as u16
                                    + ng.saturating_sub(og) as u16
                                    + nb.saturating_sub(ob) as u16
                                    + na.saturating_sub(oa) as u16;

                                [
                                    step_clamped_div(negative_change, 3),
                                    step_clamped_div(positive_change, 3),
                                    0,
                                    scaled,
                                ]
                            }
                            DiffType::Color => {
                                let dr = nr.abs_diff(or);
                                let dg = ng.abs_diff(og);
                                let db = nb.abs_diff(ob);
                                let total_difference =
                                    dr as u16 + dg as u16 + db as u16 + na.abs_diff(oa) as u16;
                                let scaled = step_clamped_div(total_difference, 3);

                                [dr, dg, db, if scaled == 0 { 255 } else { scaled }]
                            }
                        }
                    })
                    .context("Failed to write png data")?;
            }
        }

        Ok(())
    }
}

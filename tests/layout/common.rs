use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use rasterize::color::{Premultiplied, BGRA8};
use sha2::Digest;
use util::rc::{rc_static, Rc};

use crate::{
    display::{DisplayPass, PaintOpBuilder},
    layout::{self, inline::InlineContent, LayoutConstraints, LayoutContext, Point2L, Vec2L},
    raster::RasterContext,
    style::computed::HorizontalAlignment,
    text::{Face, FaceInfo, FontDb, GlyphCache},
    Subrandr,
};

macro_rules! make_tree {
    { $what: ident $(.$class: ident)+ { $($block_content: tt)* } } => {
        make_tree!(@build $what [&crate::style::ComputedStyle::DEFAULT;]; [$($class)+] { $($block_content)* })
    };
    { $what: ident $block: tt } => { make_tree!(@build $what [crate::style::ComputedStyle::DEFAULT;]; $block) };

    (@build inline [$style: expr;]; { $($content: tt)* }) => {{
        let mut builder = crate::layout::inline::InlineContentBuilder::new();
        {
            let mut root = builder.root();
            #[allow(unused)] // this is unused if the inline is empty
            let mut root = root.push_span($style.clone());
            make_tree!(@build_all inline [$style; inline=root]; $($content)*);
        }
        builder.finish()
    }};
    (@map_child inline->text $value: expr) => {
        $value
    };
    (@map_child inline->span $value: expr) => {
        $value
    };
    (@map_child inline->ruby $value: expr) => {
        $value
    };

    (@build text [$style: expr; inline=$builder: ident]; $value: literal) => {
        $builder.push_text($value)
    };
    (@build span [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        #[allow(unused)] // this is unused if the span is empty
        let mut builder = $builder.push_span($style.clone());
        make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};
    (@build ruby [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_ruby($style.clone());
        make_tree!(@build_all ruby [$style; inline=builder]; $($content)*);
    }};
    (@map_child ruby->base $value: expr) => {
        $value
    };
    (@map_child ruby->annotation $value: expr) => {
        $value
    };
    (@build base [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_base($style.clone());
        make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};
    (@build annotation [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_annotation($style.clone());
        make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};

    (@build $what: ident [$parent_style: expr; $($context_rest: tt)*]; [$($class: ident)*] { $($block_content: tt)* }) => {{
        let style = make_tree!(@apply_style $parent_style; $($class)*);
        make_tree!(@build $what [&style; $($context_rest)*]; { $($block_content)* })
    }};
    (@build $what: ident $context: tt; [] $content: tt) => {
        make_tree!(@build $what $context; $content)
    };
    (@build $what: ident $context: tt; $($content: tt)*) => {
        compile_error!(concat!(
            stringify!($what),
            " is not a valid layout tree node in this context",
        ))
    };

    // POV: You try to do anything non-trivial with a declarative macro
    //      \> TT muncher
    (
        @build_all $mapper: ident $context: tt; $($content: tt)*
    ) => {
        make_tree!(
            @build_all_rec
            $mapper $context [];
            $($content)*
        )
    };
    (
        @build_all_rec $mapper: ident $context: tt [$($result: tt)*];
        $what: ident $(.$class: ident)+ { $($block_content: tt)* } $($rest: tt)*
    ) => {
        make_tree!(@build_all_rec $mapper $context [
            $($result)*
            make_tree!(
                @map_child
                $mapper->$what
                make_tree!(@build $what $context; [$($class)+] { $($block_content)* })
            ),
        ]; $($rest)*)
    };
    (
        @build_all_rec $mapper: ident $context: tt [$($result: tt)*];
        $what: ident $block: tt $($rest: tt)*
    ) => {
        make_tree!(@build_all_rec $mapper $context [
            $($result)*
            make_tree!(
                @map_child
                $mapper->$what
                make_tree!(@build $what $context; [] $block)
            ),
        ]; $($rest)*)
    };
    (@build_all_rec $mapper: ident $context: tt $result: tt;) => { $result as [(); _] };
    (@map_child $from: ident->$to: ident $value: expr) => {
        compile_error!(concat!(
            stringify!($from),
            " does not support children of type ",
            stringify!($to)
        ))
    };

    (@apply_style $parent_style: expr; $($class: ident)+) => {{
        let mut result = $parent_style.create_derived();
        $(macros::test_apply_style!(&mut result, $class);)*
        result
    }};
    (@apply_style $parent_style: expr;) => { $parent_style.create_derived() };
}

struct TestFont {
    family_static_rc: Rc<str>,
    filename: &'static str,
    data: &'static OnceLock<&'static [u8]>,
}

impl TestFont {
    const fn family(&self) -> Rc<str> {
        unsafe { std::ptr::read(&self.family_static_rc) }
    }

    fn load(&self, assets_dir: &Path) -> Face {
        let data = *self
            .data
            .get_or_init(|| Vec::leak(std::fs::read(assets_dir.join(self.filename)).unwrap()));

        Face::load_from_static_bytes(data, 0).unwrap()
    }
}

macro_rules! test_font {
    ($name: ident, $family: literal, $path: literal) => {
        const $name: &'static TestFont = &{
            static DATA: OnceLock<&'static [u8]> = OnceLock::new();

            TestFont {
                // FIXME: erm? rustfmt?
        family_static_rc: rc_static!(str $family),
                filename: $path,
                data: &DATA,
            }
        };
    };
}

test_font!(AHEM, b"Ahem", "Ahem.ttf");
test_font!(NOTO_SERIF, b"Noto Serif", "NotoSerif-Regular.ttf");
test_font!(
    NOTO_SANS_ARABIC,
    b"Noto Sans Arabic",
    "NotoSansArabic-Regular.ttf"
);
test_font!(NOTO_SANS_JP, b"Noto Sans JP", "NotoSansJP-Regular.ttf");

const ALL_FONTS: &[&TestFont] = &[AHEM, NOTO_SERIF, NOTO_SANS_ARABIC, NOTO_SANS_JP];

test_define_style! {
    pub .ahem { font_family: rc_static!([AHEM.family()]) }
    pub .noto_serif { font_family: rc_static!([NOTO_SERIF.family()])}
    pub .noto_sans_arabic { font_family: rc_static!([NOTO_SANS_ARABIC.family()])}
    pub .noto_sans_jp { font_family: rc_static!([NOTO_SANS_JP.family()])}
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

    str::from_utf8(&output).unwrap().into()
}

pub fn check_inline(
    name: &str,
    pos: Point2L,
    viewport_size: Vec2L,
    align: HorizontalAlignment,
    inline: InlineContent,
    dpi: u32,
) {
    let project_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let tests_dir = project_dir.join("tests/");
    let assets_dir = tests_dir.join("assets/");

    let sbr = Subrandr::init();
    let mut fonts = FontDb::test(
        &sbr,
        ALL_FONTS
            .iter()
            .map(|font| FaceInfo::from_face(&font.load(&assets_dir)))
            .collect(),
    );

    let width = viewport_size.x.ceil_to_inner() as u32;
    let height = viewport_size.y.ceil_to_inner() as u32;
    let mut pixels = {
        let fragment = layout::inline::layout(
            &mut LayoutContext {
                dpi,
                fonts: &mut fonts,
            },
            &LayoutConstraints {
                size: viewport_size,
            },
            &inline,
            align,
        )
        .expect("Inline layout failed");

        let mut pixels = vec![BGRA8::ZERO; width as usize * height as usize];
        let glyph_cache = GlyphCache::new();
        let mut paint_list = Vec::new();

        DisplayPass {
            output: PaintOpBuilder(&mut paint_list),
        }
        .display_inline_content_fragment(pos, &fragment);

        let mut rasterizer = rasterize::sw::Rasterizer::new();
        let mut render_target =
            rasterize::sw::create_render_target(&mut pixels, width, height, width);

        crate::raster::rasterize_to_target(
            &mut RasterContext {
                rasterizer: &mut rasterizer,
                glyph_cache: &glyph_cache,
            },
            &mut render_target,
            &paint_list,
        )
        .expect("Fragment rasterization failed");

        pixels
    };

    let base_path = tests_dir.join("layout/snapshots/").join(name);
    let ptr_path = base_path.with_extension("png.ptr");
    let expected_pixel_hash = read_pixel_hash_from_ptr(&ptr_path);

    // Convert pixels to straight RGBA8 since that's what PNG expects.
    for pixel in &mut pixels {
        *pixel = Premultiplied(*pixel).unpremultiply();
        std::mem::swap(&mut pixel.b, &mut pixel.r);
    }

    let pixels_byte_len = pixels.len() * 4;
    let pixel_bytes =
        unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr() as *mut u8, pixels_byte_len) };
    let pixel_hash = sha2::Sha256::new().chain_update(&pixel_bytes).finalize();
    let pixel_hash_str = hex_sha256(&pixel_hash);

    let write_output_png = |file: std::fs::File| -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(pixel_bytes)?;
        writer.finish()
    };

    if expected_pixel_hash.as_deref() == Some(&pixel_hash_str) {
        let result_path = base_path.with_extension("png");
        match std::fs::File::create_new(result_path) {
            Ok(file) => write_output_png(file),
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
        std::fs::File::create(new_path)
            .map_err(Into::into)
            .and_then(write_output_png)
            .unwrap();

        if let Some(expected) = &expected_pixel_hash {
            eprintln!("Pixel hash mismatch!");
            eprintln!("Expected hash: {expected}");
            eprintln!("Current hash: {pixel_hash_str}");
        } else {
            eprintln!("No expected snapshot found for test");
        }

        let display_path = format!("snapshots/{name}.{extension}");
        eprintln!("New snapshot written to {display_path}");

        panic!()
    }
}

macro_rules! check_one {
    (
        name = $name: expr,
        $(align = $align: ident,)?
        $(dpi = $dpi: literal,)?
        $(pos = $p: tt,)?
        size = ($sx: expr, $sy: expr),

        $what: ident $(.$class: ident)+ { $($block_content: tt)* }
    ) => {{
        const TEST_ROOT_MODULE: &'static str = "layout_tests::";

        let submodule_start = module_path!().find(TEST_ROOT_MODULE).unwrap() + TEST_ROOT_MODULE.len();
        let prefix = module_path!()[submodule_start..].replace("::", "_");
        let name = format!("{prefix}_{}", $name);
        let align = check_one!(@align $($align)?);
        let dpi = check_one!(@dpi $($dpi)?);
        let pos = check_one!(@pos $($p)?);
        let size = $crate::layout::Vec2L::new(
            $crate::layout::FixedL::from_f32($sx as f32),
            $crate::layout::FixedL::from_f32($sy as f32),
        );

        use $crate::layout_tests::common::make_tree;
        let tree = make_tree!($what $(.$class)+ { $($block_content)* }) ;

        check_inline(&name, pos, size, align, tree, dpi);
    }};
    (@align $name: ident) => { $crate::style::computed::HorizontalAlignment::$name };
    (@align) => { check_one!(@align Left) };
    (@dpi $value: literal) => { $value };
    (@dpi) => { 72 };
    (@pos) => { $crate::layout::Point2L::ZERO };
    (@pos ($px: expr, $py: expr)) => {
        $crate::layout::Point2L::new(
            $crate::layout::FixedL::from_f32($px as f32),
            $crate::layout::FixedL::from_f32($py as f32),
        )
    };
    (@pos) => { $crate::layout::Point2L::ZERO };
}

macro_rules! check_test {
    (
        name = $name: ident,
        $($rest: tt)*
    ) => {
        #[test]
        fn $name() {
            $crate::layout_tests::common::check_one! {
                name = stringify!($name),
                $($rest)*
            }
        }
    };
}

pub(crate) use check_one;
pub(crate) use check_test;
pub(crate) use macros::test_define_style;
pub(crate) use make_tree;

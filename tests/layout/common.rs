use std::path::{Path, PathBuf};

use macros::test_define_style;
use rasterize::color::{Premultiplied, BGRA8};
use sha2::Digest;
use util::{
    math::I26Dot6,
    rc::{rc_static, Rc},
};

use crate::{
    layout::{self, inline::InlineContent, FixedL, LayoutConstraints, Point2L, Vec2L},
    style::computed::HorizontalAlignment,
    text::{Face, FaceInfo, FontDb},
    Renderer, Subrandr,
};

macro_rules! test_make_tree {
    { $what: ident $(.$class: ident)+ { $($block_content: tt)* } } => {
        test_make_tree!(@build $what [&crate::style::ComputedStyle::DEFAULT;]; [$($class)+] { $($block_content)* })
    };
    { $what: ident $block: tt } => { test_make_tree!(@build $what [crate::style::ComputedStyle::DEFAULT;]; $block) };

    (@build inline [$style: expr;]; { $($content: tt)* }) => {{
        let mut builder = crate::layout::inline::InlineContentBuilder::new();
        {
            let mut root = builder.root();
            let mut root = root.push_span($style.clone());
            test_make_tree!(@build_all inline [$style; inline=root]; $($content)*);
        }
        builder.finish()
    }};
    (@map_child inline->text $value: expr) => {{
        $value;
    }};
    (@map_child inline->span $value: expr) => {{
        $value;
    }};
    (@map_child inline->ruby $value: expr) => {{
        $value;
    }};

    (@build text [$style: expr; inline=$builder: ident]; $value: literal) => {{
        $builder.push_text($value);
    }};
    (@build span [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_span($style.clone());
        test_make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};
    (@build ruby [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_ruby($style.clone());
        test_make_tree!(@build_all ruby [$style; inline=builder]; $($content)*);
    }};
    (@map_child ruby->base $value: expr) => {
        $value
    };
    (@map_child ruby->annotation $value: expr) => {
        $value
    };
    (@build base [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_base($style.clone());
        test_make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};
    (@build annotation [$style: expr; inline=$builder: ident]; { $($content: tt)* }) => {{
        let mut builder = $builder.push_annotation($style.clone());
        test_make_tree!(@build_all inline [$style; inline=builder]; $($content)*);
    }};

    (@build $what: ident [$parent_style: expr; $($context_rest: tt)*]; [$($class: ident)*] { $($block_content: tt)* }) => {{
        let style = test_make_tree!(@apply_style $parent_style; $($class)*);
        test_make_tree!(@build $what [&style; $($context_rest)*]; { $($block_content)* })
    }};
    (@build $what: ident $context: tt; [] $content: tt) => {{
        test_make_tree!(@build $what $context; $content)
    }};
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
        test_make_tree!(
            @build_all_rec
            $mapper $context [];
            $($content)*
        )
    };
    (
        @build_all_rec $mapper: ident $context: tt [$($result: tt)*];
        $what: ident $(.$class: ident)+ { $($block_content: tt)* } $($rest: tt)*
    ) => {
        test_make_tree!(@build_all_rec $mapper $context [
            $($result)*
            test_make_tree!(
                @map_child
                $mapper->$what
                test_make_tree!(@build $what $context; [$($class)+] { $($block_content)* })
            ),
        ]; $($rest)*)
    };
    (
        @build_all_rec $mapper: ident $context: tt [$($result: tt)*];
        $what: ident $block: tt $($rest: tt)*
    ) => {
        test_make_tree!(@build_all_rec $mapper $context [
            $($result)*
            test_make_tree!(
                @map_child
                $mapper->$what
                test_make_tree!(@build $what $context; [] $block)
            ),
        ]; $($rest)*)
    };
    (@build_all_rec $mapper: ident $context: tt [$($result: tt)*];) => { [$($result)*] };
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

const AHEM: &[u8] = include_bytes!("../assets/Ahem.ttf");
pub const FONT_FAMILY_AHEM: Rc<[Rc<str>]> = rc_static!([rc_static!(str b"Ahem")]);

test_define_style! {
    pub .ahem {
        font_family: FONT_FAMILY_AHEM;
        font_size: I26Dot6::new(16);
    }
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

pub fn check_inline(
    name: &'static str,
    pos: Point2L,
    viewport_size: Vec2L,
    align: HorizontalAlignment,
    inline: InlineContent,
) {
    let sbr = Subrandr::init();
    let font_db = FontDb::test(
        &sbr,
        vec![FaceInfo::from_face(
            &Face::load_from_bytes(AHEM.into(), 0).unwrap(),
        )],
    );

    let mut renderer = Renderer::with_font_db(&sbr, font_db);
    renderer.set_test_layouter(move |pass| {
        let fragment = layout::inline::layout(
            pass.lctx,
            &LayoutConstraints {
                size: Vec2L::new(pass.sctx.video_width, pass.sctx.video_height),
            },
            &inline,
            align,
        )?;
        pass.emit_fragment(pos, fragment);
        Ok(())
    });

    let width = viewport_size.x.ceil_to_inner() as u32;
    let height = viewport_size.y.ceil_to_inner() as u32;
    let mut pixels = vec![BGRA8::ZERO; width as usize * height as usize];
    renderer
        .render(
            &crate::SubtitleContext {
                dpi: 72,
                video_width: viewport_size.x,
                video_height: viewport_size.y,
                padding_left: FixedL::ZERO,
                padding_right: FixedL::ZERO,
                padding_top: FixedL::ZERO,
                padding_bottom: FixedL::ZERO,
            },
            0,
            &mut pixels,
            width,
            height,
            width,
        )
        .unwrap();

    let project_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let base_path = project_dir.join("tests/layout_tests/").join(name);
    let ptr_path = base_path.with_extension("png.ptr");
    let expected_pixel_hash = read_pixel_hash_from_ptr(&ptr_path);

    let pixels_byte_len = pixels.len() * 4;
    let pixel_bytes =
        unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr() as *mut u8, pixels_byte_len) };
    let pixel_hash = sha2::Sha256::new().chain_update(&pixel_bytes).finalize();
    let pixel_hash_str = util::hex::encode_to_string(&pixel_hash);

    if expected_pixel_hash.as_deref() != Some(&pixel_hash_str) {
        let new_path = base_path.with_extension("new.png");
        let mut encoder =
            png::Encoder::new(std::fs::File::create(new_path).unwrap(), width, height);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().unwrap();
        for chunk in pixel_bytes.as_chunks_mut::<4>().0 {
            let straight = Premultiplied(BGRA8::from_bytes(*chunk)).unpremultiply();
            *chunk = [straight.r, straight.g, straight.b, straight.a];
        }
        writer.write_image_data(pixel_bytes).unwrap();
        writer.finish().unwrap();

        if let Some(expected) = expected_pixel_hash {
            eprintln!("Pixel hash mismatch!");
            eprintln!("Expected hash: {expected}");
            eprintln!("Current hash: {pixel_hash_str}");
        } else {
            eprintln!("No expected snapshot found for test");
        }
        eprintln!("New snapshot written to {name}.new.png");
        eprintln!("Commit new snapshot with <TODO>");

        panic!()
    }
}

macro_rules! layout_test {
    ($name: ident, $width: literal x $height: literal, {
        block[$($block_style: tt)*] $block_content: tt
    }) => {
        #[test]
        fn $name() {
            check_block(
                crate::layout::Point2L::ZERO,
                layout_tree! {
                    block[
                        font_family=crate::layout_tests::common::FONT_FAMILY_AHEM,
                        font_size=util::math::I26Dot6::new(16),
                        $($block_style)*
                    ] $block_content
                },
                crate::layout::Vec2L::new(crate::layout::FixedL::new($width), crate::layout::FixedL::new($height)),
                stringify!($name)
            )
        }
    };
}

pub(crate) use test_make_tree;

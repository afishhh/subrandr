use std::{path::Path, sync::OnceLock};

use rasterize::{
    color::{to_straight_rgba, Premultiplied, BGRA8},
    scene::SceneNode,
    Rasterizer,
};
use util::rc::{rc_static, Rc};

use crate::{
    display::DisplayPass,
    layout::{
        self, block::BlockContainer, inline::InlineContent, LayoutConstraints, LayoutContext,
        Point2L, Vec2L,
    },
    text::{Face, FaceInfo, FontDb, GlyphCache},
    Subrandr,
};

macro_rules! make_tree {
    { $what: ident $(.$class: ident)+ { $($block_content: tt)* } } => {
        make_tree!(@build $what [&crate::style::ComputedStyle::DEFAULT;]; [$($class)+] { $($block_content)* })
    };
    { $what: ident $block: tt } => { make_tree!(@build $what [crate::style::ComputedStyle::DEFAULT;]; $block) };

    (@build inline [$style: expr;]; { $($content: tt)* }) => {{
        let mut builder = crate::layout::inline::InlineContentBuilder::new($style.clone());
        {
            #[allow(unused)] // this is unused if the inline is empty
            let mut root = builder.root();
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
    (@map_child inline->block $value: expr) => {
        $value
    };
    (@build_all_result_ty inline) => { () };

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
    (@build block [$style: expr; inline=$builder: ident]; $content_block: tt) => {{
        $builder.push_inline_block(
            crate::layout::block::BlockContainer {
                style: $style.clone(),
                content: make_tree!(@build_block_content [$style;] $content_block)
            }
        );
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
    (@build_all_result_ty ruby) => { () };

    (@build block [$style: expr;]; $content_block: tt) => {{
        crate::layout::block::BlockContainer {
            style: $style.clone(),
            content: make_tree!(@build_block_content [$style;] $content_block)
        }
    }};
    (@build_block_content [$style: expr;] { inline $(.$class: ident)* { $($content: tt)* } }) => {
        crate::layout::block::BlockContainerContent::Inline(
            make_tree!(@build inline $(.$class)*  [$style;]; { $($content)* })
        )
    };
    (@build_block_content [$style: expr;] { $($content: tt)* }) => {
        crate::layout::block::BlockContainerContent::Block(
            make_tree!(@build_all block [$style;]; $($content)*).into()
        )
    };
    (@map_child block->block $value: expr) => {
        $value
    };
    (@build_all_result_ty block) => { crate::layout::block::BlockContainer };

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
    (@build_all_rec $mapper: ident $context: tt $result: tt;) => { $result as [make_tree!(@build_all_result_ty $mapper); _] };
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
test_font!(
    NOTO_COLOR_EMOJI,
    b"Noto Color Emoji",
    "NotoColorEmoji-Subset.ttf"
);

const ALL_FONTS: &[&TestFont] = &[
    AHEM,
    NOTO_SERIF,
    NOTO_SANS_ARABIC,
    NOTO_SANS_JP,
    NOTO_COLOR_EMOJI,
];

test_define_style! {
    pub .ahem { font_family: rc_static!([AHEM.family()]) }
    pub .noto_serif { font_family: rc_static!([NOTO_SERIF.family()])}
    pub .noto_sans_arabic { font_family: rc_static!([NOTO_SANS_ARABIC.family()])}
    pub .noto_sans_jp { font_family: rc_static!([NOTO_SANS_JP.family()])}
    pub .noto_color_emoji { font_family: rc_static!([NOTO_COLOR_EMOJI.family()])}
}

fn check_fn(
    name: &str,
    viewport_size: Vec2L,
    dpi: u32,
    fun: impl FnOnce(&mut LayoutContext, &LayoutConstraints, &mut Vec<SceneNode>),
) {
    let project_dir = test_util::project_dir();
    let tests_dir = project_dir.join("tests/");
    let assets_dir = tests_dir.join("assets/");

    let sbr = Subrandr::init();
    let log = sbr.root_logger.new_ctx();
    let mut fonts = FontDb::test(
        &log,
        ALL_FONTS
            .iter()
            .map(|font| FaceInfo::from_face(&font.load(&assets_dir)))
            .collect(),
    );

    let width = viewport_size.x.ceil_to_inner() as u32;
    let height = viewport_size.y.ceil_to_inner() as u32;
    let mut pixels = {
        let mut scene = Vec::new();
        fun(
            &mut LayoutContext {
                log: &log,
                dpi,
                fonts: &mut fonts,
            },
            &LayoutConstraints {
                size: viewport_size,
            },
            &mut scene,
        );

        let mut pixels = vec![Premultiplied(BGRA8::ZERO); width as usize * height as usize];
        let glyph_cache = GlyphCache::new();
        let mut rasterizer = rasterize::sw::Rasterizer::new();
        let mut render_target =
            rasterize::sw::RenderTarget::new(&mut pixels, width, height, width).into();

        rasterizer
            .render_scene(&mut render_target, &scene, &glyph_cache)
            .expect("Fragment rasterization failed");

        pixels
    };

    let pixel_bytes = to_straight_rgba(&mut pixels);
    test_util::check_png_snapshot(
        &tests_dir.join("layout/snapshots/").join(name),
        &format!("snapshots/{name}"),
        pixel_bytes,
        width,
        height,
    );
}

pub fn check_inline(
    name: &str,
    pos: Point2L,
    viewport_size: Vec2L,
    dpi: u32,
    inline: InlineContent,
) {
    check_fn(name, viewport_size, dpi, |lctx, constraints, output| {
        let fragment =
            layout::inline::layout(lctx, constraints, &inline).expect("Inline layout failed");

        DisplayPass::new(output, dpi).display_inline_content_fragment(pos, &fragment);
    })
}

pub fn check_block(
    name: &str,
    pos: Point2L,
    viewport_size: Vec2L,
    dpi: u32,
    block: BlockContainer,
) {
    check_fn(name, viewport_size, dpi, |lctx, constraints, output| {
        let fragment = layout::block::layout(lctx, constraints, &block).expect("Layout failed");

        DisplayPass::new(output, dpi).display_block_container_fragment(pos, &fragment);
    })
}

macro_rules! check_one {
    (
        name = $name: expr,
        $(dpi = $dpi: literal,)?
        $(pos = $p: tt,)?
        size = ($sx: expr, $sy: expr),

        $what: ident $(.$class: ident)+ { $($block_content: tt)* }
    ) => {{
        const TEST_ROOT_MODULE: &'static str = "layout_tests::";

        let submodule_start = module_path!().find(TEST_ROOT_MODULE).unwrap() + TEST_ROOT_MODULE.len();
        let prefix = module_path!()[submodule_start..].replace("::", "_");
        let name = format!("{prefix}_{}", $name);
        let dpi = check_one!(@dpi $($dpi)?);
        let pos = check_one!(@pos $($p)?);
        let size = $crate::layout::Vec2L::new(
            $crate::layout::FixedL::from_f32($sx as f32),
            $crate::layout::FixedL::from_f32($sy as f32),
        );

        use $crate::layout_tests::common::make_tree;
        let tree = make_tree!($what $(.$class)+ { $($block_content)* }) ;

        check_one!(@check $what (&name, pos, size, dpi, tree));
    }};
    (@check inline $args: tt) => { check_inline $args; };
    (@check block $args: tt) => { check_block $args; };
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

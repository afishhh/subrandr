use std::{
    cell::UnsafeCell,
    ffi::{c_char, c_int},
    mem::ManuallyDrop,
    rc::Rc,
};

use once_cell::unsync::OnceCell;
use rasterize::{
    color::{Premultiplied, BGRA8},
    scene::{Scene, SceneBuilder},
    sw,
};
use util::math::{Rect2, Vec2};

use crate::{
    capi::{instanced_raster::CInstancedRasterPass, library::CLibrary, CError, ErrorKind},
    display::DisplayPass,
    layout::{
        self,
        block::{BlockContainer, BlockContainerContent},
        image::Image,
        inline::{InlineContentBuilder, InlineRubyBuilder, InlineSpanBuilder},
        Axes, FixedL, IndependentBox, IndependentBoxFragment, LayoutConstraint, LayoutContext,
        Point2L, UserContainerBuilder, Vec2L,
    },
    style::{ComputedStyle, ComputedStyleInner},
    text::{FontDb, GlyphCache},
};

use super::instanced_raster::CInstancedRasterPassContext;

#[unsafe(no_mangle)]
extern "C" fn sbr_computed_style_default(lctx: *const CLayoutContext) -> *const ComputedStyleInner {
    assert!(!lctx.is_null());

    ComputedStyle::DEFAULT.into_raw()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_ref(style: *const ComputedStyleInner) {
    let style = ManuallyDrop::new(unsafe { ComputedStyle::from_raw(style) });
    std::mem::forget(ComputedStyle::clone(&style));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_unref(style: *const ComputedStyleInner) {
    drop(unsafe { ComputedStyle::from_raw(style) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_compute_from_str(
    lctx: *mut CLayoutContext,
    declarations: *const c_char,
    declarations_len: usize,
    parent: *const ComputedStyleInner,
) -> *const ComputedStyleInner {
    let lib = &*(*lctx).lib;

    let source = ctry!(std::str::from_utf8(std::slice::from_raw_parts(
        declarations.cast::<u8>(),
        declarations_len,
    )));
    let parent = ManuallyDrop::new(ComputedStyle::from_raw(parent));

    let buffer = ctry!(crate::csssyn::buffer::TokenBuffer::from_source(source));
    let parsed_declarations =
        crate::csssyn::algorithms::parse_declaration_list(buffer.start()).collect::<Vec<_>>();

    ComputedStyle::into_raw(crate::style::compute_with_declarations(
        &lib.root_logger.new_ctx(),
        &mut std::iter::once(&parsed_declarations[..]),
        &parent,
    ))
}

#[derive(Debug)]
struct CImage {
    texture: sw::Texture<'static>,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_image_from_bgra(
    _lib: *const CLibrary,
    data: *const u32,
    width: u32,
    height: u32,
    stride: u32,
) -> *const CImage {
    // Since `FixedL` is 26.6 we won't be able to handle images larger than like 2^25
    // but that'd be in a scenario where your scene contains nothing but the image so
    // a sane limit is actually lower, let's arbitrarily pick 2^22.
    const LIMIT: u32 = 1 << 22;

    if width > LIMIT {
        cthrow!(InvalidArgument, "image width greater than limit");
    } else if height > LIMIT {
        cthrow!(InvalidArgument, "image height greater than limit");
    }

    let pixels = std::slice::from_raw_parts(data.cast(), stride as usize * height as usize);
    Rc::into_raw(Rc::new(CImage {
        texture: sw::Texture::from_strided_bgra(pixels, width, height, stride),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_image_ref(texture: *const CImage) {
    Rc::increment_strong_count(texture);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_image_unref(texture: *const CImage) {
    Rc::decrement_strong_count(texture);
}

impl layout::image::ImageInner for CImage {
    fn display(&self, builder: &mut rasterize::scene::SceneContentBuilder, size: Vec2L) {
        builder.bitmap(
            self.texture.clone().into(),
            util::math::Vec2::new(
                size.x.round_to_inner() as u32,
                size.y.round_to_inner() as u32,
            ),
            None,
            BGRA8::WHITE,
        );
    }
}

struct CLayoutContext {
    lib: *const CLibrary,
    font_db: FontDb,
    glyph_cache: GlyphCache,
    rasterizer: sw::Rasterizer,
    dpi: u32,

    in_layout_pass: bool,

    // NOTE: self-referencial to `display_scene_builder` but should never
    //       be `Some` once this is dropped
    display_pass: Option<DisplayPass<'static>>,
    display_scene_builder: SceneBuilder,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_context_create(lib: *const CLibrary) -> *mut CLayoutContext {
    Box::into_raw(Box::new(CLayoutContext {
        font_db: ctry!(FontDb::new(&(*lib).root_logger.new_ctx())),
        lib,
        glyph_cache: GlyphCache::new(),
        rasterizer: sw::Rasterizer::new(),
        dpi: 72,

        in_layout_pass: false,

        display_pass: None,
        display_scene_builder: SceneBuilder::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_context_set_dpi(lctx: *mut CLayoutContext, dpi: u32) -> c_int {
    if dpi == 0 {
        cthrow!(InvalidArgument, "dpi must be greater than zero");
    }

    (*lctx).dpi = dpi;

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_context_destroy(lctx: *mut CLayoutContext) {
    drop(unsafe { Box::from_raw(lctx) });
}

struct CBox {
    inner: IndependentBox,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_box_from_image(
    _lctx: *mut CLayoutContext,
    image: *mut CImage,
    style: *const ComputedStyleInner,
) -> *mut CBox {
    Box::into_raw(Box::new(CBox {
        inner: IndependentBox::Image(Image {
            style: (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
            natural_dimensions: layout::image::NaturalDimensions {
                // TODO: handle overflow
                width: FixedL::new((*image).texture.width() as i32),
                height: FixedL::new((*image).texture.height() as i32),
            },
            inner: Rc::from_raw(image),
        }),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_box_destroy(block: *mut CBox) {
    drop(Box::from_raw(block));
}

struct CBlockBuilder {
    style: ComputedStyle,
    contents: Vec<IndependentBox>,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_create(
    _lctx: *mut CLayoutContext,
    style: *const ComputedStyleInner,
) -> *mut CBlockBuilder {
    Box::into_raw(Box::new(CBlockBuilder {
        style: (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
        contents: Vec::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_destroy(builder: *mut CBlockBuilder) {
    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_push(builder: *mut CBlockBuilder, block: *mut CBox) {
    (*builder).contents.push(Box::from_raw(block).inner);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_set_style(
    builder: *mut CBlockBuilder,
    style: *const ComputedStyleInner,
) {
    // TODO: msg
    assert!(
        (*builder).contents.is_empty(),
        "`sbr_block_builder_set_style` called once an active root builder has already been created"
    );

    (*builder).style = (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_finish(builder: *mut CBlockBuilder) -> *mut CBox {
    Box::into_raw(Box::new(CBox {
        inner: IndependentBox::Block(BlockContainer {
            style: (*builder).style.clone(),
            content: BlockContainerContent::Block(std::mem::take(&mut (*builder).contents)),
        }),
    }))
}

struct CInlineBuilder {
    style: ComputedStyle,
    // NOTE: This field is self-referential to `inner`, must come before in drop order.
    root: OnceCell<UnsafeCell<InlineSpanBuilder<'static>>>,
    inner: InlineContentBuilder,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_create(
    _lctx: *mut CLayoutContext,
    style: *const ComputedStyleInner,
) -> *mut CInlineBuilder {
    let style = (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone();
    Box::into_raw(Box::new(CInlineBuilder {
        inner: InlineContentBuilder::new(style.create_derived()),
        style,
        root: OnceCell::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_set_style(
    builder: *mut CInlineBuilder,
    style: *const ComputedStyleInner,
) {
    assert!((*builder).root.get().is_none(), "`sbr_inline_builder_set_style` called once an active root builder has already been created");

    (*builder).style = (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone();
    (*builder)
        .inner
        .set_root_style((*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_destroy(builder: *mut CInlineBuilder) {
    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_root(
    builder: *mut CInlineBuilder,
) -> *mut InlineSpanBuilder<'static> {
    (*builder)
        .root
        .get_or_init(|| UnsafeCell::new((*builder).inner.root()))
        .get()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_finish_block(builder: *mut CInlineBuilder) -> *mut CBox {
    drop((*builder).root.take());

    let result = Box::into_raw(Box::new(CBox {
        inner: IndependentBox::Block(BlockContainer {
            style: (*builder).style.clone(),
            content: BlockContainerContent::Inline((*builder).inner.finish()),
        }),
    }));
    (*builder).inner.set_root_style((*builder).style.clone());
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_append_text(
    builder: *mut InlineSpanBuilder<'static>,
    text: *const c_char,
    text_len: usize,
) -> c_int {
    let text = ctry!(std::str::from_utf8(std::slice::from_raw_parts(
        text.cast::<u8>(),
        text_len,
    )));

    (*builder).push_text(text);

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_push_atomic(
    builder: *mut InlineSpanBuilder<'static>,
    block: *mut CBox,
) {
    (*builder).push_atomic(Box::from_raw(block).inner);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_push_span(
    builder: *mut InlineSpanBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new((*builder).push_span(
        (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
    )))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_push_ruby(
    builder: *mut InlineSpanBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineRubyBuilder<'static> {
    Box::into_raw(Box::new((*builder).push_ruby(
        (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
    )))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_finish(builder: *mut InlineSpanBuilder<'static>) {
    assert!(
        !(*builder).is_root(),
        "`sbr_span_builder_finish` called on root builder"
    );

    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_ruby_builder_push_base(
    builder: *mut InlineRubyBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new((*builder).push_base(
        (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
    )))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_ruby_builder_push_annotation(
    builder: *mut InlineRubyBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new((*builder).push_annotation(
        (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
    )))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_ruby_builder_finish(builder: *mut InlineRubyBuilder<'static>) {
    drop(Box::from_raw(builder));
}

struct CLayoutPass(());

impl CLayoutPass {
    #[track_caller]
    unsafe fn ensure(lpass: *mut CLayoutPass) -> *mut CLayoutContext {
        let lctx = lpass.cast::<CLayoutContext>();

        assert!(
            (*lctx).in_layout_pass,
            "invalid layout pass: associated context does not currently have an active layout pass"
        );

        lctx
    }

    #[track_caller]
    unsafe fn with_core_lctx<T>(
        lpass: *mut CLayoutPass,
        fun: impl FnOnce(&mut LayoutContext) -> T,
    ) -> T {
        let lctx = Self::ensure(lpass);

        fun(&mut LayoutContext {
            log: &(*(*lctx).lib).root_logger.new_ctx(),
            dpi: (*lctx).dpi,
            fonts: &mut (*lctx).font_db,
            initial_containing_block_size: Vec2L::ZERO,
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_pass_begin(lctx: *mut CLayoutContext) -> *mut CLayoutPass {
    (*lctx).in_layout_pass = true;
    lctx.cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_pass_end(lpass: *mut CLayoutPass) {
    let lctx = CLayoutPass::ensure(lpass);
    (*lctx).in_layout_pass = false;
}

struct MeasureFlags {
    measure_axes: Axes,
}

impl MeasureFlags {
    fn parse(flags: u64) -> Result<Self, CError> {
        const MEASURE_WIDTH: u64 = 1 << 0;
        const MEASURE_HEIGHT: u64 = 1 << 1;
        const KNOWN_MASK: u64 = MEASURE_WIDTH | MEASURE_HEIGHT;

        if flags & !KNOWN_MASK != 0 {
            return Err(CError::new(
                ErrorKind::InvalidArgument,
                "unknown bits set in `sbr_box_measure` flags",
            ));
        }

        let mut result = MeasureFlags {
            measure_axes: Axes::NONE,
        };

        if flags & MEASURE_WIDTH != 0 {
            result.measure_axes.x = true;
        }
        if flags & MEASURE_HEIGHT != 0 {
            result.measure_axes.y = true;
        }

        Ok(result)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_box_measure(
    cbox: *mut CBox,
    lpass: *mut CLayoutPass,
    out: *mut Vec2L,
    constraints: Vec2L,
    flags: u64,
) -> c_int {
    let MeasureFlags { measure_axes } = ctry!(MeasureFlags::parse(flags));

    let constraints = Vec2::new(
        LayoutConstraint::Fixed(constraints.x),
        LayoutConstraint::Fixed(constraints.y),
    );

    let result = ctry!(CLayoutPass::with_core_lctx(lpass, |lctx| {
        (*cbox)
            .inner
            .layout_initial(lctx)?
            .measure(lctx, constraints, measure_axes)
    }));
    out.write(result);

    0
}

struct CFragment {
    inner: IndependentBoxFragment,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_box_layout(
    cbox: *mut CBox,
    lpass: *mut CLayoutPass,
    available_size: Vec2L,
) -> *mut CFragment {
    let fragment = ctry!(CLayoutPass::with_core_lctx(lpass, |lctx| {
        Box::from_raw(cbox).inner.layout(lctx, available_size)
    }));

    Box::into_raw(Box::new(CFragment { inner: fragment }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_fragment_size(fragment: *mut CFragment) -> Vec2L {
    (*fragment).inner.fbox().margin_box().size()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_fragment_destroy(fragment: *mut CFragment) {
    drop(Box::from_raw(fragment));
}

struct CDisplayPass(());

impl CDisplayPass {
    #[track_caller]
    unsafe fn ensure<'a>(
        pass: *mut CDisplayPass,
    ) -> (*mut CLayoutContext, &'a mut DisplayPass<'static>) {
        let lctx = pass.cast::<CLayoutContext>();

        let Some(pass) = (*lctx).display_pass.as_mut() else {
            panic!(
                "invalid display pass: associated context does not currently have an active display pass"
            );
        };

        (lctx, pass)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_display_pass_begin(
    lctx: *mut CLayoutContext,
    dpi: u32,
) -> *mut CLayoutPass {
    assert!(
        (*lctx).display_pass.is_none(),
        "display pass already in progress"
    );

    (*lctx).display_scene_builder.reset();
    (*lctx).display_pass = Some(DisplayPass::new(
        (*lctx).display_scene_builder.root(),
        dpi,
        &(*lctx).glyph_cache,
        &mut (*lctx).rasterizer,
    ));

    lctx.cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_fragment_display(
    fragment: *mut CFragment,
    dpass: *mut CDisplayPass,
    offset: Point2L,
) -> c_int {
    let (_, dstate) = CDisplayPass::ensure(dpass);

    ctry!((*dstate).display_independent_box_fragment(offset, &(*fragment).inner));

    0
}

struct CScene {
    inner: Scene,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_display_pass_finish(dpass: *mut CDisplayPass) -> *mut CScene {
    let (lctx, _) = CDisplayPass::ensure(dpass);

    assert!((*lctx).display_pass.take().is_some());

    Box::into_raw(Box::new(CScene {
        inner: (*lctx).display_scene_builder.finish(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_scene_destroy(scene: *mut CScene) {
    drop(Box::from_raw(scene))
}

struct CSwRasterizer {
    lib: *const CLibrary,
    inner: sw::Rasterizer,

    instanced_raster_pass: CInstancedRasterPass,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_sw_rasterizer_create(lib: *const CLibrary) -> *mut CSwRasterizer {
    Box::into_raw(Box::new(CSwRasterizer {
        lib,
        inner: sw::Rasterizer::new(),
        instanced_raster_pass: CInstancedRasterPass::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_sw_rasterizer_bad_render_dont_commit(
    rasterizer: *mut CSwRasterizer,
    scene: *const CScene,
    buffer: *mut Premultiplied<BGRA8>,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let buffer = std::slice::from_raw_parts_mut(buffer, stride as usize * height as usize);
    ctry!((*rasterizer).inner.render_scene(
        &(*(*rasterizer).lib).root_logger.new_ctx(),
        &mut rasterize::sw::RenderTarget::new(buffer, width, height, stride),
        &(*scene).inner,
    ));

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_sw_rasterizer_render_instanced(
    rasterizer: *mut CSwRasterizer,
    scene: *const CScene,
    clip_rect: Rect2<i32>,
    flags: u64,
) -> *mut CInstancedRasterPass {
    ctry!((*rasterizer).instanced_raster_pass.render_scene(
        &(*(*rasterizer).lib).root_logger.new_ctx(),
        &mut (*rasterizer).inner,
        &(*scene).inner,
        clip_rect,
        flags,
        CInstancedRasterPassContext::Abc(&raw mut (*rasterizer).inner),
    ));

    &raw mut (*rasterizer).instanced_raster_pass
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_sw_rasterizer_destroy(rasterizer: *mut CSwRasterizer) {
    drop(Box::from_raw(rasterizer));
}

struct CUserBuilder {
    lpass: *mut CLayoutPass,
    inner: UserContainerBuilder,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_custom_container_builder_create(
    lpass: *mut CLayoutPass,
    style: *const ComputedStyleInner,
) -> *mut CUserBuilder {
    CLayoutPass::ensure(lpass);
    Box::into_raw(Box::new(CUserBuilder {
        lpass,
        inner: UserContainerBuilder::new(
            (*ManuallyDrop::new(ComputedStyle::from_raw(style))).clone(),
        ),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_custom_container_builder_destroy(builder: *mut CUserBuilder) {
    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_custom_container_builder_place(
    builder: *mut CUserBuilder,
    offset: Vec2L,
    block: *mut CBox,
    size: Vec2L,
) -> c_int {
    ctry!(CLayoutPass::with_core_lctx((*builder).lpass, |lctx| {
        let box_inner = Box::from_raw(block).inner;
        let partial = box_inner.layout_initial(lctx)?;
        (*builder).inner.place(lctx, offset, partial, size)
    }));

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_custom_container_builder_finish(
    builder: *mut CUserBuilder,
    size: Vec2L,
) -> *mut CBox {
    Box::into_raw(Box::new(CBox {
        inner: IndependentBox::User((*builder).inner.finish(size)),
    }))
}

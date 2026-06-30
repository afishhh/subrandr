use std::{
    cell::UnsafeCell,
    ffi::{c_char, c_int},
    mem::ManuallyDrop,
    ptr::NonNull,
};

use once_cell::unsync::OnceCell;
use rasterize::{scene::SceneBuilder, sw};
use util::math::Rect2;

use crate::{
    capi::{instanced_raster::CInstancedRasterPass, library::CLibrary},
    display::DisplayPass,
    layout::{
        self,
        block::{BlockContainer, BlockContainerContent, BlockContainerFragment},
        inline::{InlineContent, InlineContentBuilder, InlineRubyBuilder, InlineSpanBuilder},
        LayoutConstraints, LayoutContext, Point2L, Vec2L,
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
    let declarations = crate::csssyn::value::parse_declaration_list(buffer.start()).collect();

    ComputedStyle::into_raw(crate::style::from_declarations(
        lib.root_logger.new_ctx(),
        declarations,
        &parent,
    ))
}

struct CLayoutContext {
    lib: *const CLibrary,
    font_db: FontDb,
    glyph_cache: GlyphCache,
    rasterizer: sw::Rasterizer,
    dpi: u32,

    in_layout_pass: bool,

    raster_pass: CInstancedRasterPass,
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

        raster_pass: CInstancedRasterPass::new(),
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

struct CBlock {
    inner: BlockContainer,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_from_inline(
    _lctx: *mut CLayoutContext,
    inline: *mut CInline,
    style: *const ComputedStyleInner,
) -> *mut CBlock {
    Box::into_raw(Box::new(CBlock {
        inner: BlockContainer {
            style: ComputedStyle::from_raw(style),
            content: BlockContainerContent::Inline(Box::from_raw(inline).inner),
        },
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_destroy(block: *mut CBlock) {
    drop(Box::from_raw(block));
}

struct CBlockBuilder {
    style: ComputedStyle,
    contents: Vec<BlockContainer>,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_create(
    _lctx: *mut CLayoutContext,
    style: *const ComputedStyleInner,
) -> *mut CBlockBuilder {
    Box::into_raw(Box::new(CBlockBuilder {
        style: ComputedStyle::from_raw(style),
        contents: Vec::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_destroy(builder: *mut CBlockBuilder) {
    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_push(builder: *mut CBlockBuilder, block: *mut CBlock) {
    (*builder).contents.push(Box::from_raw(block).inner);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_builder_finish(builder: *mut CBlockBuilder) -> *mut CBlock {
    Box::into_raw(Box::new(CBlock {
        inner: BlockContainer {
            style: (*builder).style.clone(),
            content: BlockContainerContent::Block((*builder).contents.clone()),
        },
    }))
}

struct CInline {
    inner: InlineContent,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_destroy(inline: *mut CInline) {
    drop(Box::from_raw(inline));
}

struct CInlineBuilder {
    // NOTE: This field is self-referential to `inner`, must come before in drop order.
    root: OnceCell<UnsafeCell<InlineSpanBuilder<'static>>>,
    inner: InlineContentBuilder,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_create(
    _lctx: *mut CLayoutContext,
    style: *const ComputedStyleInner,
) -> *mut CInlineBuilder {
    Box::into_raw(Box::new(CInlineBuilder {
        inner: InlineContentBuilder::new(ComputedStyle::from_raw(style)),
        root: OnceCell::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_inline_builder_set_style(
    builder: *mut CInlineBuilder,
    style: *const ComputedStyleInner,
) {
    assert!((*builder).root.get().is_none(), "`sbr_inline_builder_set_style` called once an active root builder has already been created");

    (*builder)
        .inner
        .set_root_style(ComputedStyle::from_raw(style));
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
unsafe extern "C" fn sbr_inline_builder_finish(builder: *mut CInlineBuilder) -> *mut CInline {
    drop((*builder).root.take());

    Box::into_raw(Box::new(CInline {
        inner: (*builder).inner.finish(),
    }))
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
unsafe extern "C" fn sbr_span_builder_finish(builder: *mut InlineSpanBuilder<'static>) {
    assert!(
        !(*builder).is_root(),
        "`sbr_span_builder_finish` called on root builder"
    );

    drop(Box::from_raw(builder));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_push_span(
    builder: *mut InlineSpanBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new(
        (*builder).push_span(ComputedStyle::from_raw(style)),
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_span_builder_push_ruby(
    builder: *mut InlineSpanBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineRubyBuilder<'static> {
    Box::into_raw(Box::new(
        (*builder).push_ruby(ComputedStyle::from_raw(style)),
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_ruby_builer_push_base(
    builder: *mut InlineRubyBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new(
        (*builder).push_base(ComputedStyle::from_raw(style)),
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_ruby_builer_push_annotation(
    builder: *mut InlineRubyBuilder<'static>,
    style: *const ComputedStyleInner,
) -> *mut InlineSpanBuilder<'static> {
    Box::into_raw(Box::new(
        (*builder).push_annotation(ComputedStyle::from_raw(style)),
    ))
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

struct CFragment {
    inner: BlockContainerFragment,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_block_layout(
    block: *mut CBlock,
    lpass: *mut CLayoutPass,
    available_size: Vec2L,
) -> *mut CFragment {
    let lconstraints = LayoutConstraints {
        size: available_size,
    };
    let fragment = ctry!(CLayoutPass::with_core_lctx(lpass, |lctx| {
        layout::block::layout_initial(lctx, &(*block).inner)
            .and_then(|partial| partial.layout(lctx, &lconstraints))
    }));

    Box::into_raw(Box::new(CFragment { inner: fragment }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_fragment_size(fragment: *mut CFragment) -> Vec2L {
    (*fragment).inner.fbox.size_for_layout()
}

pub(super) struct FragmentRasterPassContext(NonNull<CLayoutContext>);

impl FragmentRasterPassContext {
    pub(super) fn rasterizer(&self) -> *mut sw::Rasterizer {
        unsafe { &raw mut (*self.0.as_ptr()).rasterizer }
    }

    pub(super) fn finish(self) {}
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_fragment_render_instanced(
    fragment: *mut CFragment,
    lctx: *mut CLayoutContext,
    offset: Point2L,
    clip_rect: Rect2<i32>,
    flags: u64,
) -> *mut CInstancedRasterPass {
    let scene = {
        let mut builder = SceneBuilder::new();

        ctry!(DisplayPass::new(
            builder.root(),
            (*lctx).dpi,
            &(*lctx).glyph_cache,
            &mut (*lctx).rasterizer,
        )
        .display_block_container_fragment(offset, &(*fragment).inner));

        builder.finish()
    };

    ctry!((*lctx).raster_pass.render_scene(
        &(*(*lctx).lib).root_logger.new_ctx(),
        &mut (*lctx).rasterizer,
        &scene,
        clip_rect,
        flags,
        CInstancedRasterPassContext::Fragment(FragmentRasterPassContext(NonNull::new_unchecked(
            lctx,
        ))),
    ));

    &raw mut (*lctx).raster_pass
}

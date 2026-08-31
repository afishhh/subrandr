use core::{convert::AsRef, iter::Iterator};
use std::{marker::PhantomData, ops::Range, rc::Rc};

use icu_properties::{
    props::{GeneralCategory, VerticalOrientation},
    CodePointMapDataBorrowed,
};
use icu_segmenter::{options::LineBreakOptions, GraphemeClusterSegmenter};
use rasterize::scene::Rotation;
use thiserror::Error;
use util::math::{I26Dot6, Vec2};

use super::{
    block::{BlockContainer, BlockContainerFragment, PartialBlockContainer},
    Axes, BoxFragmentationPart, EdgeExtents, FixedL, FragmentBox, LayoutConstraint, LayoutContext,
    Vec2L, Vec2LW, Vec2W, Vec2WritingModeExt,
};
use crate::{
    style::{
        computed::{
            self, FontSlant, HorizontalAlignment, InlineSizing, TextAlign, TextOrientation,
            ToPhysicalPixels, WhiteSpaceCollapse, WritingMode,
        },
        ComputedStyle,
    },
    text::{
        self, BaselineMetrics, Direction, Font, FontMatcher, FontMetrics, OpenTypeTag,
        ShapingBuffer,
    },
};

mod glyph_string;
pub use glyph_string::*;

// This character is used to represent opaque objects nested inside inline text content,
// this includes ruby containers and `inline-block`s.
const OBJECT_REPLACEMENT_CHARACTER: char = '\u{FFFC}';
const OBJECT_REPLACEMENT_LENGTH: usize = OBJECT_REPLACEMENT_CHARACTER.len_utf8();

const GENERAL_CATEGORY_MAP: CodePointMapDataBorrowed<GeneralCategory> =
    CodePointMapDataBorrowed::new();
const VERTICAL_ORIENTATION_MAP: CodePointMapDataBorrowed<VerticalOrientation> =
    CodePointMapDataBorrowed::new();

/// A flat representation of inline content.
///
/// This structure stores a layout tree for inline content in a [`Vec`]
/// alongside an additional [`Vec`] of [`Rc<str>`]s that stores the
/// final text runs on which line breaking and bidi reordering will be
/// performed.
#[derive(Debug, Clone)]
pub struct InlineContent {
    text_runs: Box<[Rc<str>]>,
    items: Box<[InlineItem]>,
    root_style: ComputedStyle,
}

impl Default for InlineContent {
    fn default() -> Self {
        Self {
            text_runs: Box::new([Rc::from("")]),
            items: Box::default(),
            root_style: ComputedStyle::DEFAULT,
        }
    }
}

#[derive(Debug, Clone)]
enum InlineItem {
    Span(InlineSpan),
    Text(InlineText),
    Block(InlineBlock),
    SpanEnd,
}

#[derive(Debug, Clone)]
struct InlineSpan {
    style: ComputedStyle,
    kind: InlineSpanKind,
}

#[derive(Debug, Clone)]
enum InlineSpanKind {
    Span,
    // Contents are interleaved base-annotation pairs of kind `RubyInternal`.
    Ruby {
        content_index: usize,
    },
    RubyInternal {
        run_index: usize,
        outer_style: ComputedStyle,
    },
}

#[derive(Debug, Clone)]
struct InlineText {
    content_range: Range<usize>,
}

#[derive(Debug, Clone)]
struct InlineBlock {
    content_index: usize,
    block: Box<BlockContainer>,
}

mod builder;
pub use builder::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baseline {
    Alphabetic,
    Central,
    IdeographicUnder,
    IdeographicOver,
}

impl Baseline {
    pub fn metrics(self, metrics: &FontMetrics, vertical: bool) -> BaselineMetrics {
        let alphabetic = metrics.alphabetic_baseline;
        let central = metrics.central_baseline;
        match (self, vertical) {
            (Baseline::Alphabetic, false) => alphabetic,
            (Baseline::Central, false) => {
                let mid = alphabetic.height() / 2;

                BaselineMetrics {
                    ascender: mid,
                    descender: -mid,
                }
            }
            (Baseline::IdeographicUnder, false) => BaselineMetrics {
                ascender: alphabetic.height(),
                descender: FixedL::ZERO,
            },
            (Baseline::IdeographicOver, false) => BaselineMetrics {
                ascender: FixedL::ZERO,
                descender: -alphabetic.height(),
            },

            (Baseline::IdeographicUnder, true) => BaselineMetrics {
                ascender: central.height(),
                descender: FixedL::ZERO,
            },
            (Baseline::IdeographicOver, true) => BaselineMetrics {
                ascender: FixedL::ZERO,
                descender: -central.height(),
            },
            (Baseline::Central, true) => metrics.central_baseline,
            // TODO: Yes this can produce bad results but even the spec doesn't
            //       know what to do here:
            //   https://drafts.csswg.org/css-inline-3/#issue-2ffa7534
            //   https://github.com/w3c/csswg-drafts/issues/5424
            (Baseline::Alphabetic, true) => metrics.central_baseline,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BoxBaselineSet {
    alphabetic: FixedL,
    central: FixedL,
    ideographic_under: FixedL,
    ideographic_over: FixedL,
}

impl BoxBaselineSet {
    const ZERO: Self = Self {
        alphabetic: FixedL::ZERO,
        central: FixedL::ZERO,
        ideographic_under: FixedL::ZERO,
        ideographic_over: FixedL::ZERO,
    };

    fn new(metrics: &FontMetrics, writing_mode: WritingMode) -> Self {
        let vertical = writing_mode.is_typographic_mode_vertical();
        let alphabetic_offset = Baseline::Alphabetic
            .metrics(metrics, vertical)
            .block_start_offset(writing_mode);

        Self {
            alphabetic: alphabetic_offset,
            central: Baseline::Central
                .metrics(metrics, vertical)
                .block_start_offset(writing_mode)
                - alphabetic_offset,
            ideographic_under: Baseline::IdeographicUnder
                .metrics(metrics, vertical)
                .block_start_offset(writing_mode)
                - alphabetic_offset,
            ideographic_over: Baseline::IdeographicOver
                .metrics(metrics, vertical)
                .block_start_offset(writing_mode)
                - alphabetic_offset,
        }
    }

    pub fn offset(&self, amount: FixedL) -> Self {
        Self {
            alphabetic: self.alphabetic + amount,
            ..*self
        }
    }

    pub fn get(&self, baseline: Baseline) -> FixedL {
        match baseline {
            Baseline::Alphabetic => self.alphabetic,
            Baseline::Central => self.central + self.alphabetic,
            Baseline::IdeographicUnder => self.ideographic_under + self.alphabetic,
            Baseline::IdeographicOver => self.ideographic_over + self.alphabetic,
        }
    }
}

#[derive(Debug)]
pub struct SpanFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub content: OffsetInlineItemFragmentVec,
}

#[derive(Debug, Clone)]
pub struct TextFragment {
    pub style: ComputedStyle,
    pub glyphs: GlyphString,
    pub inline_size: FixedL,
    pub align_to: Baseline,
    vertical_typesetting: Option<VerticalTypesetting>,
}

impl TextFragment {
    pub fn glyph_transform(&self) -> Rotation {
        match self.vertical_typesetting {
            Some(VerticalTypesetting::Sideways { clockwise: true }) => Rotation::Clockwise90,
            Some(VerticalTypesetting::Sideways { clockwise: false }) => {
                Rotation::CounterClockwise90
            }
            Some(VerticalTypesetting::Upright) | None => Rotation::None,
        }
    }

    pub fn is_typographic_mode_vertical(&self) -> bool {
        self.vertical_typesetting.is_typographic_mode_vertical()
    }

    pub fn is_vertical(&self) -> bool {
        self.vertical_typesetting.is_some()
    }
}

#[derive(Debug)]
pub struct RubyFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub content: Vec<(Vec2L, RubyBaseFragment, Vec2L, RubyAnnotationFragment)>,
}

#[derive(Debug)]
pub struct RubyBaseFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub struct RubyAnnotationFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub baseline_block_offset: FixedL,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub enum InlineItemFragment {
    Span(SpanFragment),
    Text(TextFragment),
    Ruby(RubyFragment),
    Block(BlockContainerFragment),
}

type OffsetInlineItemFragmentVec = Vec<(Vec2L, util::rc::Rc<InlineItemFragment>)>;

#[derive(Debug, Clone)]
pub struct LineBoxFragment {
    pub fbox: FragmentBox,
    pub dominant_baseline_offset: FixedL,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug, Clone)]
pub struct InlineContentFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    // https://drafts.csswg.org/css-align-3/#baseline-export
    pub line_baselines: BoxBaselineSet,
    // NOTE: due to const reasons this can't be `Font`
    pub primary_font_metrics: FontMetrics,
    pub lines: Vec<(Vec2L, util::rc::Rc<LineBoxFragment>)>,
}

impl InlineContentFragment {
    pub const EMPTY: Self = Self {
        fbox: FragmentBox::ZERO,
        style: ComputedStyle::DEFAULT,
        line_baselines: BoxBaselineSet::ZERO,
        primary_font_metrics: FontMetrics::ZERO,
        lines: Vec::new(),
    };
}

#[derive(Debug, Error)]
pub enum InlineLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::SelectError),
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error(transparent)]
    FreeType(#[from] text::FreeTypeError),
    #[error("Orthogonal flows are not supported yet")]
    OrthogonalFlow,
}

#[derive(Debug)]
struct InitialShapingResult<'c> {
    shaped: Vec<ShapedItem<'c, PartialStage>>,
    break_opportunities: Vec<usize>,
    text_leaf_items: Vec<LeafItemRange<'c>>,
    bidi: unicode_bidi::BidiInfo<'c>,
    font_feature_events: Vec<FontFeatureEvent>,
    grapheme_cluster_boundaries: Vec<usize>,
}

impl InitialShapingResult<'_> {
    fn empty() -> Self {
        Self {
            shaped: Vec::new(),
            break_opportunities: Vec::new(),
            text_leaf_items: Vec::new(),
            bidi: unicode_bidi::BidiInfo::new("", None),
            font_feature_events: Vec::new(),
            grapheme_cluster_boundaries: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct FragmentShapingResult<'partial, 'content> {
    initial: &'partial InitialShapingResult<'content>,
    items: Vec<ShapedItem<'content, FragmentStage<'partial>>>,
}

// TODO: How should reordering affect padding fragmentation?
//       Is the current implementation correct? (everything in visual order)
/// Holds per-span state prepared during shaping and used during further layout to
/// calculate span fragmentation.
#[derive(Debug, Clone)]
struct SpanState<'a> {
    style: &'a ComputedStyle,
    primary_font: Font,
    remaining_content_bytes: u32,
    remaining_line_content_bytes: u32,
    seen_first: bool,
    parent: usize,
}

impl<'a> SpanState<'a> {
    fn new(style: &'a ComputedStyle, primary_font: Font, parent: usize) -> Self {
        Self {
            style,
            primary_font,
            remaining_content_bytes: 0,
            remaining_line_content_bytes: 0,
            seen_first: false,
            parent,
        }
    }

    fn walk_up(states: &mut [Self], mut span_id: usize, mut callback: impl FnMut(&mut Self)) {
        while span_id != usize::MAX {
            let state = &mut states[span_id];
            callback(state);
            span_id = state.parent;
        }
    }
}

#[derive(Debug, Clone)]
struct LeafItemRange<'a> {
    range: Range<usize>,
    span_id: usize,
    style: &'a ComputedStyle,
}

impl AsRef<Range<usize>> for LeafItemRange<'_> {
    fn as_ref(&self) -> &Range<usize> {
        &self.range
    }
}

pub fn slice_sorted_ranges_intersecting<E: AsRef<Range<usize>>>(
    ranges: &[E],
    range: Range<usize>,
) -> &[E] {
    let start = match ranges.binary_search_by_key(&range.start, |e| e.as_ref().end) {
        Ok(s) => s + 1,
        Err(s) => s,
    };
    let end = match ranges[start..].binary_search_by_key(&range.end, |e| e.as_ref().start) {
        Ok(s) => s,
        Err(s) => s,
    } + start;

    &ranges[start..end]
}

trait LayoutStage<'content> {
    type Block;
    type RubyInner: std::fmt::Debug;
}

#[derive(Debug)] // these have Debug only to satisfy Debug derive
struct PartialStage;

impl<'content> LayoutStage<'content> for PartialStage {
    type Block = PartialBlockContainer<'content>;
    type RubyInner = InitialShapingResult<'content>;
}

#[derive(Debug)]
struct FragmentStage<'partial>(PhantomData<&'partial ()>);

impl<'partial, 'content: 'partial> LayoutStage<'content> for FragmentStage<'partial> {
    type Block = BlockItemFragment;
    type RubyInner = FragmentShapingResult<'partial, 'content>;
}

#[derive(Debug)]
struct ShapedItem<'c, S: LayoutStage<'c>> {
    range: Range<usize>,
    kind: ShapedItemKind<'c, S>,
    /// Padding metrics used during line breaking, note that due to bidi
    /// reordering this *may not correspond to the final padding* applied
    /// to these glyphs. In fact, since shaped items don't even correspond
    /// to particular spans, this should be entirely ignored as soon as we
    /// leave line breaking!
    spacing: ShapedItemSpacing,
}

#[derive(Debug, Clone)]
struct ShapedItemSpacing {
    current_spacing_left: FixedL,
    current_spacing_right: FixedL,
}

impl ShapedItemSpacing {
    // Basically placeholder values for when we don't care about this anymore but
    // need to construct a `ShapedItem`.
    // Must only be used after line-breaking when this information is no longer
    // necessary.
    const MAX: Self = Self {
        current_spacing_left: FixedL::MAX,
        current_spacing_right: FixedL::MAX,
    };

    fn fragment_break(&mut self) -> Self {
        let remainder = Self {
            current_spacing_left: FixedL::ZERO,
            ..*self
        };
        self.current_spacing_right = FixedL::ZERO;
        remainder
    }
}

#[derive(Debug)]
enum ShapedItemKind<'c, S: LayoutStage<'c>> {
    Text(TextItem),
    Ruby(RubyItem<'c, S>),
    Block(BlockItem<'c, S>),
}

#[derive(Debug, Clone)]
struct TextItem {
    font_matcher: FontMatcher,
    primary_font: Font,
    glyphs: GlyphString,
    break_after: bool,
    vertical_typesetting: Option<VerticalTypesetting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerticalTypesetting {
    Upright,
    Sideways { clockwise: bool },
}

trait VerticalTypesettingExt {
    fn is_typographic_mode_vertical(&self) -> bool;
}

impl VerticalTypesettingExt for Option<VerticalTypesetting> {
    fn is_typographic_mode_vertical(&self) -> bool {
        matches!(self, Some(VerticalTypesetting::Upright))
    }
}

impl TextItem {
    fn is_typographic_mode_vertical(&self) -> bool {
        self.vertical_typesetting.is_typographic_mode_vertical()
    }
}

impl text::Glyph {
    fn inline_advance(&self, typesetting: Option<VerticalTypesetting>) -> FixedL {
        match typesetting {
            Some(VerticalTypesetting::Upright) => -self.y_advance,
            Some(VerticalTypesetting::Sideways { .. }) | None => self.x_advance,
        }
    }
}

#[derive(Debug)]
struct RubyItem<'c, S: LayoutStage<'c>> {
    style: ComputedStyle,
    base_annotation_pairs: Vec<(RubyItemBase<'c, S>, RubyItemAnnotation<'c, S>)>,
    span_id: usize,
}

#[derive(Debug)]
struct RubyItemBase<'c, S: LayoutStage<'c>> {
    style: &'c ComputedStyle,
    primary_font: Font,
    inner: S::RubyInner,
}

#[derive(Debug)]
struct RubyItemAnnotation<'c, S: LayoutStage<'c>> {
    style: &'c ComputedStyle,
    primary_font: Font,
    inner: S::RubyInner,
}

struct BlockItem<'c, S: LayoutStage<'c>> {
    span_id: usize,
    inner: S::Block,
}

impl<'c, S: LayoutStage<'c>> std::fmt::Debug for BlockItem<'c, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapedItemBlock")
            .field("span_id", &self.span_id)
            .finish_non_exhaustive()
    }
}

struct BlockItemFragment {
    dominant_baseline_block_offset: FixedL,
    inline_size: FixedL,
    fragment: BlockContainerFragment,
}

impl BlockItemFragment {
    fn accumulate_width(&self, result: &mut FixedL) {
        *result += self.inline_size;
    }
}

impl FragmentShapingResult<'_, '_> {
    fn accumulate_width(&self, result: &mut FixedL) {
        for item in &self.items {
            item.accumulate_width(result);
        }
    }
}

impl BlockItemFragment {
    fn layout_partial(
        lctx: &mut LayoutContext,
        partial: &PartialBlockContainer,
        constraints: Vec2<LayoutConstraint>,
        writing_mode: WritingMode,
        dominant_baseline: Baseline,
    ) -> Result<Self, InlineLayoutError> {
        let sizes = partial.inline_level_block_sizes(lctx, constraints, writing_mode)?;
        let available_block_space = match constraints.block(writing_mode) {
            LayoutConstraint::Fixed(fixed) => Some(fixed),
            LayoutConstraint::MaxContent => None,
        };
        let fragment = partial.layout(
            lctx,
            sizes.size,
            sizes.margins(writing_mode),
            available_block_space,
            writing_mode,
        )?;

        Ok(Self {
            inline_size: fragment.fbox.inline_size(writing_mode),
            dominant_baseline_block_offset: fragment.baselines(writing_mode).map_or_else(
                || {
                    let block_size = fragment.fbox.block_size(writing_mode);

                    // https://www.w3.org/TR/css-inline-3/#baseline-synthesis-box
                    match dominant_baseline {
                        Baseline::Alphabetic | Baseline::IdeographicUnder => block_size,
                        Baseline::Central => block_size / 2,
                        Baseline::IdeographicOver => FixedL::ZERO,
                    }
                },
                |set| set.get(dominant_baseline),
            ),
            fragment,
        })
    }
}

impl<'content> ShapedItemKind<'content, PartialStage> {
    fn to_fragment_item<'partial>(
        &'partial self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        writing_mode: WritingMode,
        dominant_baseline: Baseline,
    ) -> Result<ShapedItemKind<'content, FragmentStage<'partial>>, InlineLayoutError> {
        Ok(match &self {
            ShapedItemKind::Text(text) => ShapedItemKind::Text(text.clone()),
            ShapedItemKind::Ruby(ruby) => ShapedItemKind::Ruby(RubyItem {
                style: ruby.style.clone(),
                span_id: ruby.span_id,
                base_annotation_pairs: ruby
                    .base_annotation_pairs
                    .iter()
                    .map(
                        |(base, annotation)| -> Result<
                            (
                                RubyItemBase<'content, FragmentStage<'partial>>,
                                RubyItemAnnotation<'content, FragmentStage<'partial>>,
                            ),
                            InlineLayoutError,
                        > {
                            Ok((
                                RubyItemBase {
                                    style: base.style,
                                    primary_font: base.primary_font.clone(),
                                    inner: base.inner.to_fragment_result(
                                        lctx,
                                        constraints,
                                        writing_mode,
                                        dominant_baseline,
                                    )?,
                                },
                                RubyItemAnnotation {
                                    style: annotation.style,
                                    primary_font: annotation.primary_font.clone(),
                                    inner: annotation.inner.to_fragment_result(
                                        lctx,
                                        constraints,
                                        writing_mode,
                                        dominant_baseline,
                                    )?,
                                },
                            ))
                        },
                    )
                    .collect::<Result<_, InlineLayoutError>>()?,
            }),
            ShapedItemKind::Block(block) => ShapedItemKind::Block(BlockItem {
                span_id: block.span_id,
                inner: BlockItemFragment::layout_partial(
                    lctx,
                    &block.inner,
                    constraints,
                    writing_mode,
                    dominant_baseline,
                )?,
            }),
        })
    }
}

impl<'content> InitialShapingResult<'content> {
    fn to_fragment_result(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        writing_mode: WritingMode,
        dominant_baseline: Baseline,
    ) -> Result<FragmentShapingResult<'_, 'content>, InlineLayoutError> {
        let items = self
            .shaped
            .iter()
            .map(|item| {
                Ok(ShapedItem {
                    range: item.range.clone(),
                    kind: item.kind.to_fragment_item(
                        lctx,
                        constraints,
                        writing_mode,
                        dominant_baseline,
                    )?,
                    spacing: item.spacing.clone(),
                })
            })
            .collect::<Result<_, InlineLayoutError>>()?;

        Ok(FragmentShapingResult {
            initial: self,
            items,
        })
    }
}

fn font_matcher_from_style(style: &ComputedStyle, lctx: &mut LayoutContext) -> FontMatcher {
    text::FontMatcher::new(
        style.font_family().clone(),
        text::FontStyle {
            weight: style.font_weight(),
            italic: match style.font_slant() {
                FontSlant::Regular => false,
                FontSlant::Italic => true,
            },
        },
        style.font_size(),
        lctx.dpi,
    )
}

fn primary_font_from_style(
    style: &ComputedStyle,
    lctx: &mut LayoutContext,
) -> Result<Font, InlineLayoutError> {
    font_matcher_from_style(style, lctx)
        .primary(lctx.log, lctx.fonts)
        .map_err(Into::into)
}

#[derive(Debug)]
struct FontFeatureEvent {
    utf8_index: usize,
    kind: FontFeatureEventKind,
}

#[derive(Debug)]
enum FontFeatureEventKind {
    Set(OpenTypeTag, u32),
    Reset,
}

struct RunShaper<'a> {
    buffer: &'a mut ShapingBuffer,
    font_feature_events: &'a [FontFeatureEvent],
    grapheme_cluster_boundaries: &'a [usize],
}

impl RunShaper<'_> {
    fn set_buffer_content(
        &mut self,
        text: &str,
        range: Range<usize>,
        direction: Direction,
        vertical_typesetting: Option<VerticalTypesetting>,
    ) {
        self.buffer.clear();
        self.buffer.set_direction(direction);
        self.buffer.set_pre_context(&text[..range.start]);

        // https://drafts.csswg.org/css-writing-modes-4/#vertical-font-features
        // TODO: Disallow setting VERT and VRTR manually if vertical typesetting is enabled?
        //       Not sure whether "must be enabled" means it also can't be manually disabled
        match vertical_typesetting {
            Some(VerticalTypesetting::Upright) => {
                self.buffer.set_feature(OpenTypeTag::FEAT_VERT, 1);
            }
            Some(VerticalTypesetting::Sideways { .. }) => {
                self.buffer.set_feature(OpenTypeTag::FEAT_VRTR, 1);
            }
            None => (),
        }

        let next_grapheme_boundary_idx =
            match self.grapheme_cluster_boundaries.binary_search(&range.start) {
                Ok(i) => i + 1,
                Err(i) => i,
            };
        let mut next_grapheme_boundary_it = self.grapheme_cluster_boundaries
            [next_grapheme_boundary_idx..]
            .iter()
            .copied();

        let mut next_feature_ev_idx = 'feature_idx: {
            let mut idx = match self
                .font_feature_events
                .binary_search_by_key(&range.start, |f| f.utf8_index)
            {
                Ok(i) => i,
                Err(i) => match i.checked_sub(1) {
                    Some(prev) => prev,
                    None => break 'feature_idx i,
                },
            };

            let initial_cluster_utf8_index = self.font_feature_events[idx].utf8_index;
            while let Some(prev_idx) = idx.checked_sub(1) {
                if self.font_feature_events[prev_idx].utf8_index != initial_cluster_utf8_index {
                    break;
                }

                idx = prev_idx;
            }

            idx
        };

        let mut current = range.start;
        while current != range.end {
            loop {
                match self.font_feature_events.get(next_feature_ev_idx) {
                    Some(event) if event.utf8_index <= current => {
                        match event.kind {
                            FontFeatureEventKind::Set(tag, value) => {
                                self.buffer.set_feature(tag, value)
                            }
                            FontFeatureEventKind::Reset => self.buffer.reset_features(),
                        }

                        next_feature_ev_idx += 1;
                    }
                    _ => break,
                }
            }

            let end = next_grapheme_boundary_it
                .next()
                .map_or(range.end, |end| end.min(range.end));
            self.buffer.add_grapheme(&text[current..end], current);
            current = end;
        }

        self.buffer.set_post_context(&text[range.end..]);
    }

    pub fn shape(
        &mut self,
        output: &mut dyn text::ShapingSink,
        font_iterator: text::FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
    ) -> Result<(), text::ShapingError> {
        self.buffer
            .shape(lctx.log, output, font_iterator, lctx.fonts)
    }
}

impl BaselineMetrics {
    fn block_start_offset(self, writing_mode: WritingMode) -> FixedL {
        if !writing_mode.is_line_flipped() {
            self.ascender
        } else {
            -self.descender
        }
    }

    fn block_end_offset(self, writing_mode: WritingMode) -> FixedL {
        if !writing_mode.is_line_flipped() {
            self.descender
        } else {
            -self.ascender
        }
    }

    fn height(&self) -> FixedL {
        self.ascender - self.descender
    }
}

impl WritingMode {
    // https://drafts.csswg.org/css-writing-modes-4/#text-baselines
    // our `text-orientation` is always `mixed` for now.
    fn auto_dominant_baseline(self) -> Baseline {
        // In vertical typographic mode,
        if self.is_typographic_mode_vertical() {
            // the central baseline is used as the dominant baseline when text-orientation is mixed or upright.
            Baseline::Central
        } else {
            // Otherwise the alphabetic baseline is used.
            Baseline::Alphabetic
        }
    }

    pub(crate) fn is_typographic_mode_horizontal(self) -> bool {
        match self {
            WritingMode::HorizontalTtb => true,
            WritingMode::VerticalLtr | WritingMode::VerticalRtl => false,
            WritingMode::SidewaysRtl => true,
        }
    }

    fn is_typographic_mode_vertical(self) -> bool {
        !self.is_typographic_mode_horizontal()
    }

    pub(crate) fn is_block_reversed(self) -> bool {
        matches!(self, WritingMode::VerticalRtl | WritingMode::SidewaysRtl)
    }

    pub(crate) fn is_line_flipped(self) -> bool {
        self.is_vertical()
    }
}

struct TextSegmenter {
    vertical_typesetting: Option<VerticalTypesetting>,
    range: Range<usize>,
    current_bidi_paragraph: usize,
    next_grapheme_end_idx: usize,
    was_newline: bool,
}

struct TextSegment {
    bidi_level: unicode_bidi::Level,
    vertical_typesetting: Option<VerticalTypesetting>,
    range: Range<usize>,
    followed_by_newline: bool,
}

impl TextSegmenter {
    fn new() -> Self {
        Self {
            vertical_typesetting: None,
            range: 0..0,
            current_bidi_paragraph: 0,
            next_grapheme_end_idx: 0,
            was_newline: false,
        }
    }

    fn skip_to(&mut self, bidi: &unicode_bidi::BidiInfo, index: usize) {
        self.range.start = index;
        self.range.end = index;
        match bidi
            .paragraphs
            .binary_search_by_key(&self.range.start, |p| p.range.start)
        {
            Ok(i) => i,
            Err(i) => i - 1,
        };
    }

    fn take(&mut self, bidi: &unicode_bidi::BidiInfo) -> TextSegment {
        debug_assert!(!self.range.is_empty());

        let segment = TextSegment {
            bidi_level: bidi.levels[self.range.start],
            vertical_typesetting: self.vertical_typesetting,
            range: self.range.start..self.range.end - usize::from(self.was_newline),
            followed_by_newline: self.was_newline,
        };
        self.range.start = self.range.end;
        if bidi.paragraphs[self.current_bidi_paragraph].range.end == self.range.end {
            self.current_bidi_paragraph += 1;
        }
        segment
    }

    fn segment_bidi(
        &mut self,
        bidi: &unicode_bidi::BidiInfo,
        run_text: &str,
        until: usize,
    ) -> Option<TextSegment> {
        assert!(until <= bidi.levels.len() && until <= run_text.len());

        let mut current_level = bidi.levels[self.range.start];
        while self.range.end < until {
            let level = bidi.levels[self.range.end];
            let paragraph_ended =
                bidi.paragraphs[self.current_bidi_paragraph].range.end == self.range.end;
            let level_changed_or_break = current_level != level || self.was_newline;
            if !self.range.is_empty() && (paragraph_ended || level_changed_or_break) {
                return Some(self.take(bidi));
            }
            current_level = level;
            self.was_newline = run_text.as_bytes()[self.range.end] == b'\n';
            self.range.end += 1;
        }

        None
    }

    fn segment_full(
        &mut self,
        bidi: &unicode_bidi::BidiInfo,
        run_text: &str,
        until: usize,
        grapheme_cluster_boundaries: &[usize],
        writing_mode: WritingMode,
        text_orientation: TextOrientation,
    ) -> Option<TextSegment> {
        // https://drafts.csswg.org/css-writing-modes-3/#text-orientation
        // This property specifies the orientation of text within a line.
        // Current values only have an effect in vertical typographic modes: the property has no effect in horizontal typographic modes.
        if writing_mode.is_horizontal() {
            self.segment_bidi(bidi, run_text, until)
        } else if writing_mode.is_typographic_mode_horizontal() {
            let vertical_typesetting = match writing_mode {
                WritingMode::HorizontalTtb
                | WritingMode::VerticalRtl
                | WritingMode::VerticalLtr => unreachable!(),
                WritingMode::SidewaysRtl => VerticalTypesetting::Sideways { clockwise: true },
            };

            if !self.range.is_empty() && self.vertical_typesetting != Some(vertical_typesetting) {
                return Some(self.take(bidi));
            }
            self.vertical_typesetting = Some(vertical_typesetting);

            self.segment_bidi(bidi, run_text, until)
        } else {
            match text_orientation {
                computed::TextOrientation::Mixed => (),
            }

            if grapheme_cluster_boundaries
                .get(self.next_grapheme_end_idx)
                .is_some_and(|&x| x <= self.range.end)
            {
                self.next_grapheme_end_idx =
                    match grapheme_cluster_boundaries.binary_search(&self.range.end) {
                        Ok(i) => i + 1,
                        Err(i) => i,
                    };
            }

            while self.range.end != until {
                let end = grapheme_cluster_boundaries
                    .get(self.next_grapheme_end_idx)
                    .copied()
                    .map_or(until, |end| end.min(until));
                let grapheme = &run_text[self.range.end..end];

                // https://www.unicode.org/reports/tr50/#grapheme_clusters
                // If the cluster contains an enclosing combining mark (general category Me), then the whole cluster has the Vertical_Orientation property value U.
                let unicode_orientation = if grapheme
                    .chars()
                    .any(|c| GENERAL_CATEGORY_MAP.get(c) == GeneralCategory::EnclosingMark)
                {
                    VerticalOrientation::Upright
                } else {
                    VERTICAL_ORIENTATION_MAP.get(grapheme.chars().next().unwrap())
                };

                // (back to CSS spec)
                // typesetting it upright if its orientation property is U, Tu, or Tr; or typesetting it sideways (90° clockwise from horizontal) if its orientation property is R.
                let vertical_typesetting = if unicode_orientation == VerticalOrientation::Upright
                    || unicode_orientation == VerticalOrientation::TransformedUpright
                    || unicode_orientation == VerticalOrientation::TransformedRotated
                {
                    VerticalTypesetting::Upright
                } else {
                    VerticalTypesetting::Sideways { clockwise: true }
                };

                if !self.range.is_empty() && self.vertical_typesetting != Some(vertical_typesetting)
                {
                    return Some(self.take(bidi));
                }
                self.vertical_typesetting = Some(vertical_typesetting);

                self.next_grapheme_end_idx += 1;

                if let Some(segment) = self.segment_bidi(bidi, run_text, end) {
                    return Some(segment);
                }
            }

            None
        }
    }
}

fn shape_run_initial<'a>(
    content: &'a InlineContent,
    run_index: usize,
    item_index: usize,
    end_item_index: &mut usize,
    lctx: &mut LayoutContext,
    compute_break_opportunities: bool,
    span_state: &mut Vec<SpanState<'a>>,
    inner_style: &'a ComputedStyle,
) -> Result<InitialShapingResult<'a>, InlineLayoutError> {
    struct ShapedItemBuilder<'c, 's, 'l, 'll> {
        content: &'c InlineContent,
        run_text: &'c Rc<str>,
        bidi: unicode_bidi::BidiInfo<'c>,
        grapheme_cluster_boundaries: Vec<usize>,
        lctx: &'l mut LayoutContext<'ll>,
        writing_mode: WritingMode,

        break_opportunities: Vec<usize>,
        shaped: Vec<ShapedItem<'c, PartialStage>>,
        span_state: &'s mut Vec<SpanState<'c>>,
        shaping_buffer: ShapingBuffer,
        queued_text_matcher: FontMatcher,
        queued_text_segmenter: TextSegmenter,
        font_feature_events: Vec<FontFeatureEvent>,
        queued_spacing: FixedL,
        current_span_id: usize,
        total_content_bytes_added: usize,
    }

    struct SpanStackEntry<'a> {
        parent_style: &'a ComputedStyle,
        span_content_start: usize,
    }

    impl<'a> ShapedItemBuilder<'a, '_, '_, '_> {
        fn push_break_opportunity(&mut self, idx: usize) {
            if let Some(&previous) = self.break_opportunities.last() {
                if previous == idx {
                    return;
                }

                debug_assert!(previous < idx);
            }

            self.break_opportunities.push(idx);
        }

        /// Compute additional break opportunities required for compatibility.
        ///
        /// This is required because `icu_segmenter` computes break opportunities in
        /// accordance with the Unicode Line Breaking Algorithm but UAs can introduce
        /// ones not covered by it (which users might end up relying on).
        ///
        /// One example is: in `&ZeroWidthSpace; &ZeroWidthSpace;` there *is* a break
        /// opportunity after the normal space because CSS only considers `' '` and `'\t'`
        /// for break opportunities.
        ///
        /// In the case of `white-space-collapse: preserve` the above case could also be
        /// explained by CSS [White Space Collapsing and Transformation Rules] specifying
        /// a soft break opportunity after any run of spaces but browsers seem to exhibit
        /// this behavior unconditionally.
        ///
        /// [White Space Collapsing and Transformation Rules]: https://www.w3.org/TR/css-text-3/#white-space-phase-1
        fn compute_extra_whitespace_break_opportunities(
            &mut self,
            range: Range<usize>,
            style: &ComputedStyle,
        ) {
            match style.white_space_collapse() {
                WhiteSpaceCollapse::Collapse
                | WhiteSpaceCollapse::PreserveBreaks
                | WhiteSpaceCollapse::Preserve => {
                    let bytes = self.run_text.as_bytes();
                    let mut in_space_run = false;
                    for i in range {
                        if matches!(bytes[i], b' ' | b'\t') {
                            in_space_run = true;
                        } else {
                            if in_space_run {
                                self.push_break_opportunity(i);
                            }
                            in_space_run = false;
                        }
                    }
                }
            }
        }

        fn compute_text_break_opportunities(&mut self, range: Range<usize>, style: &ComputedStyle) {
            // FIXME: This makes sense conceptually but may fall apart in the presence of
            //        dictionary line segmenters. Some testing has to be done to make sure
            //        this produces correct results.
            //        (for now we can also just not care since I can't even get Firefox
            //         or Chromium to do dictionary based breaking...)
            let padded_start_grapheme_index =
                match self.grapheme_cluster_boundaries.binary_search(&range.start) {
                    Ok(found) => found.saturating_sub(1),
                    Err(left) => left - 1,
                };
            let padded_start = self.grapheme_cluster_boundaries[padded_start_grapheme_index];
            let padded_end = self
                .grapheme_cluster_boundaries
                .get(
                    match self.grapheme_cluster_boundaries[padded_start_grapheme_index..]
                        .binary_search(&range.end)
                    {
                        Ok(found) => padded_start_grapheme_index + found + 1,
                        Err(left) => padded_start_grapheme_index + left + 1,
                    },
                )
                .copied()
                .unwrap_or(self.run_text.len());

            let segmenter = icu_segmenter::LineSegmenter::new_auto({
                let mut options = LineBreakOptions::default();
                options.strictness = Some(style.line_break());
                options.word_option = Some(style.word_break());
                options
            });

            let ignore_after = range.end.min(self.run_text.len() - 1);
            let iter = segmenter
                .segment_str(&self.run_text[padded_start..padded_end])
                .map(|idx| idx + padded_start);

            let mut last = range.start;
            for idx in iter {
                // The first breaks are going to be either at the start of the string or
                // inside our "padding" look-behind character, both of which we want to ignore.
                if idx < range.start {
                    continue;
                }

                self.compute_extra_whitespace_break_opportunities(last..idx, style);
                last = idx;

                if idx > ignore_after {
                    break;
                }

                self.push_break_opportunity(idx);
            }
        }

        fn push_text_segment(
            &mut self,
            font_matcher: FontMatcher,
            TextSegment {
                bidi_level,
                vertical_typesetting,
                range,
                followed_by_newline,
            }: TextSegment,
        ) -> Result<(), InlineLayoutError> {
            let direction = match (vertical_typesetting, bidi_level.is_ltr()) {
                (Some(VerticalTypesetting::Sideways { .. }) | None, true) => Direction::Ltr,
                (Some(VerticalTypesetting::Sideways { .. }) | None, false) => Direction::Rtl,
                (Some(VerticalTypesetting::Upright), true) => Direction::Ttb,
                (Some(VerticalTypesetting::Upright), false) => Direction::Btt,
            };

            let shaper = &mut RunShaper {
                buffer: &mut self.shaping_buffer,
                font_feature_events: &self.font_feature_events,
                grapheme_cluster_boundaries: &self.grapheme_cluster_boundaries,
            };
            let glyphs = {
                shaper.buffer.guess_properties();
                shaper.set_buffer_content(
                    self.run_text,
                    range.clone(),
                    direction,
                    vertical_typesetting,
                );
                let mut output = GlyphString::new(self.run_text.clone(), direction);
                shaper.shape(&mut output, font_matcher.iterator(), self.lctx)?;
                output
            };
            shaper.buffer.clear();

            self.shaped.push(ShapedItem {
                range: range.clone(),
                kind: ShapedItemKind::Text(TextItem {
                    primary_font: font_matcher.primary(self.lctx.log, self.lctx.fonts)?,
                    font_matcher,
                    glyphs,
                    break_after: followed_by_newline,
                    vertical_typesetting,
                }),
                spacing: ShapedItemSpacing {
                    current_spacing_left: self.queued_spacing,
                    current_spacing_right: FixedL::ZERO,
                },
            });
            self.queued_spacing = FixedL::ZERO;

            Ok(())
        }

        fn flush_queued_text(&mut self) -> Result<(), InlineLayoutError> {
            if !self.queued_text_segmenter.range.is_empty() {
                let segment = self.queued_text_segmenter.take(&self.bidi);
                self.push_text_segment(self.queued_text_matcher.clone(), segment)?;
            }

            Ok(())
        }

        fn handle_span_start(&mut self, style: &'a ComputedStyle) -> Result<(), InlineLayoutError> {
            let left_spacing = style
                .inline_min_margin(self.writing_mode)
                .to_physical_pixels(self.lctx.dpi)
                .unwrap_or(FixedL::ZERO)
                + style
                    .inline_min_padding(self.writing_mode)
                    .to_physical_pixels(self.lctx.dpi);

            if left_spacing != FixedL::ZERO {
                // NOTE: When thinking about this padding system, one may stumble upon the consideration:
                //       "what if some segment of text needs to have different (cloned) padding but we
                //        want to shape it along with some preceeding one" or similar.
                //       This cannot happen precisely because any change in padding parameters will also
                //       trigger a shaping break.
                //       The only exception is right-side cloned padding which needs to be communicated
                //       via a side-channel because it may differ inside a single `ShapedItem`.
                self.flush_queued_text()?;

                self.queued_spacing += left_spacing;
            }

            let next_span_id = self.span_state.len();
            self.span_state.push(SpanState::new(
                style,
                primary_font_from_style(style, self.lctx)?,
                self.current_span_id,
            ));
            self.current_span_id = next_span_id;

            Ok(())
        }

        fn handle_span_end(
            &mut self,
            style: &ComputedStyle,
            entry: &SpanStackEntry,
        ) -> Result<(), InlineLayoutError> {
            let state = &mut self.span_state[self.current_span_id];
            state.remaining_content_bytes =
                (self.total_content_bytes_added - entry.span_content_start) as u32;
            self.current_span_id = state.parent;

            if state.remaining_content_bytes == 0 {
                // FIXME: Padding for spans that have no leaf items is currently ignored.
                //        Some experimentation suggests that browsers tie such spans to the
                //        character immediately preceeding them, thus it should be possible
                //        to place them in an empty leaf text item or something and then fix
                //        the "no glyphs" case on text branch reconstruction.
                let left_spacing = style
                    .inline_min_margin(self.writing_mode)
                    .to_physical_pixels(self.lctx.dpi)
                    .unwrap_or(FixedL::ZERO)
                    + style
                        .inline_min_padding(self.writing_mode)
                        .to_physical_pixels(self.lctx.dpi);
                self.queued_spacing -= left_spacing;
                return Ok(());
            };

            let right_spacing = style
                .inline_max_padding(self.writing_mode)
                .to_physical_pixels(self.lctx.dpi)
                + style
                    .inline_max_margin(self.writing_mode)
                    .to_physical_pixels(self.lctx.dpi)
                    .unwrap_or(FixedL::ZERO);

            if right_spacing != FixedL::ZERO {
                self.flush_queued_text()?;

                if let Some(item) = self.shaped.last_mut() {
                    item.spacing.current_spacing_right += right_spacing;
                }
            }

            Ok(())
        }

        fn process_items(
            mut self,
            item_index: usize,
            end_item_index: &mut usize,
            compute_break_opportunities: bool,
            inner_style: &'a ComputedStyle,
        ) -> Result<InitialShapingResult<'a>, InlineLayoutError> {
            let writing_mode = self.content.root_style.writing_mode();
            let items = &self.content.items;
            let mut current_item = item_index;
            let mut current_style = inner_style;
            let mut span_stack: Vec<SpanStackEntry> = Vec::new();
            let mut text_leaf_items = Vec::new();

            while let Some(item) = items
                .get(current_item)
                .filter(|_| !span_stack.is_empty() || current_item < *end_item_index)
            {
                current_item += 1;
                match item {
                    InlineItem::Span(span) => match span.kind {
                        InlineSpanKind::Span | InlineSpanKind::RubyInternal { .. } => {
                            // TODO: Neither the margin, padding, border properties nor the any properties that do not apply to inline boxes apply to base containers or annotation containers. Additionally, line-height does not apply to annotation containers.
                            // No browser seems to respect this, also this statement is
                            // very weird since padding *does* apply to inline boxes so
                            // I have no clue what's going on in the standard here.
                            self.handle_span_start(&span.style)?;
                            span_stack.push(SpanStackEntry {
                                parent_style: current_style,
                                span_content_start: self.total_content_bytes_added,
                            });
                            current_style = &span.style;
                        }
                        InlineSpanKind::Ruby { content_index } => {
                            self.flush_queued_text()?;

                            let content_end = content_index + OBJECT_REPLACEMENT_LENGTH;
                            self.shaped.push(ShapedItem {
                                range: content_index..content_end,
                                kind: ShapedItemKind::Ruby(RubyItem {
                                    style: span.style.clone(),
                                    span_id: self.current_span_id,
                                    base_annotation_pairs: {
                                        let mut result = Vec::new();

                                        while !matches!(items[current_item], InlineItem::SpanEnd) {
                                            let &InlineItem::Span(InlineSpan {
                                                kind:
                                                    InlineSpanKind::RubyInternal {
                                                        run_index,
                                                        outer_style: ref base_style,
                                                    },
                                                ..
                                            }) = &items[current_item]
                                            else {
                                                unreachable!("Illegal ruby base item");
                                            };

                                            let base = RubyItemBase {
                                                style: base_style,
                                                primary_font: primary_font_from_style(
                                                    base_style, self.lctx,
                                                )?,
                                                inner: shape_run_initial(
                                                    self.content,
                                                    run_index,
                                                    current_item,
                                                    {
                                                        current_item += 1;
                                                        &mut current_item
                                                    },
                                                    self.lctx,
                                                    false,
                                                    self.span_state,
                                                    base_style,
                                                )?,
                                            };
                                            let annotation = if !matches!(
                                                items[current_item],
                                                InlineItem::SpanEnd
                                            ) {
                                                let &InlineItem::Span(InlineSpan {
                                                    kind:
                                                        InlineSpanKind::RubyInternal {
                                                            run_index,
                                                            outer_style: ref annotation_style,
                                                        },
                                                    ..
                                                }) = &items[current_item]
                                                else {
                                                    unreachable!("Illegal ruby annotation item");
                                                };

                                                let result = shape_run_initial(
                                                    self.content,
                                                    run_index,
                                                    current_item,
                                                    {
                                                        current_item += 1;
                                                        &mut current_item
                                                    },
                                                    self.lctx,
                                                    false,
                                                    self.span_state,
                                                    annotation_style,
                                                )?;
                                                RubyItemAnnotation {
                                                    style: annotation_style,
                                                    primary_font: primary_font_from_style(
                                                        annotation_style,
                                                        self.lctx,
                                                    )?,
                                                    inner: result,
                                                }
                                            } else {
                                                RubyItemAnnotation {
                                                    style: const { &ComputedStyle::DEFAULT },
                                                    primary_font: primary_font_from_style(
                                                        &ComputedStyle::DEFAULT,
                                                        self.lctx,
                                                    )?,
                                                    inner: InitialShapingResult::empty(),
                                                }
                                            };

                                            result.push((base, annotation));
                                        }

                                        // For the ruby container's `SpanEnd`
                                        current_item += 1;

                                        result
                                    },
                                }),
                                spacing: ShapedItemSpacing {
                                    current_spacing_left: self.queued_spacing,
                                    current_spacing_right: FixedL::ZERO,
                                },
                            });
                            self.queued_spacing = FixedL::ZERO;
                            self.total_content_bytes_added += OBJECT_REPLACEMENT_LENGTH;

                            if compute_break_opportunities {
                                if content_index != 0 {
                                    self.push_break_opportunity(content_index);
                                }
                                if content_end != self.run_text.len() {
                                    self.push_break_opportunity(content_end);
                                }
                            }
                        }
                    },
                    InlineItem::Text(text) => {
                        let font_matcher = font_matcher_from_style(current_style, self.lctx);

                        if self.queued_text_segmenter.range.end != text.content_range.start {
                            // Whatever non-text content we're jumping over should've flushed the
                            // queued text and the segmenter should now be empty.
                            debug_assert!(self.queued_text_segmenter.range.is_empty());
                            self.queued_text_segmenter
                                .skip_to(&self.bidi, text.content_range.start);
                        }
                        if self.queued_text_matcher != font_matcher {
                            self.flush_queued_text()?;
                            self.queued_text_matcher = font_matcher;
                        }

                        while let Some(segment) = self.queued_text_segmenter.segment_full(
                            &self.bidi,
                            self.run_text,
                            text.content_range.end,
                            &self.grapheme_cluster_boundaries,
                            writing_mode,
                            current_style.text_orientation(),
                        ) {
                            self.push_text_segment(self.queued_text_matcher.clone(), segment)?;
                        }

                        let font_feature_settings = current_style.font_feature_settings();
                        for (tag, value) in font_feature_settings.iter() {
                            self.font_feature_events.push(FontFeatureEvent {
                                utf8_index: text.content_range.start,
                                kind: FontFeatureEventKind::Set(tag, value),
                            });
                        }

                        if !font_feature_settings.is_empty() {
                            self.font_feature_events.push(FontFeatureEvent {
                                utf8_index: text.content_range.end,
                                kind: FontFeatureEventKind::Reset,
                            });
                        }

                        text_leaf_items.push(LeafItemRange {
                            range: text.content_range.clone(),
                            span_id: self.current_span_id,
                            style: current_style,
                        });
                        // HACK: This feels hacky but we need to make sure gets done here
                        //       without requiring that the queued text gets flushed.
                        self.total_content_bytes_added += self.run_text[text.content_range.clone()]
                            .bytes()
                            .filter(|&b| b != b'\n')
                            .count();

                        if compute_break_opportunities {
                            self.compute_text_break_opportunities(
                                text.content_range.clone(),
                                current_style,
                            );
                        }
                    }
                    &InlineItem::Block(InlineBlock {
                        content_index,
                        ref block,
                    }) => {
                        self.flush_queued_text()?;

                        let content_end = content_index + OBJECT_REPLACEMENT_LENGTH;
                        self.shaped.push(ShapedItem {
                            range: content_index..content_end,
                            kind: ShapedItemKind::Block(BlockItem {
                                span_id: self.current_span_id,
                                inner: super::block::layout_initial(self.lctx, block)?,
                            }),
                            spacing: ShapedItemSpacing {
                                current_spacing_left: self.queued_spacing,
                                current_spacing_right: FixedL::ZERO,
                            },
                        });
                        self.queued_spacing = FixedL::ZERO;
                        self.total_content_bytes_added += OBJECT_REPLACEMENT_LENGTH;

                        if compute_break_opportunities {
                            if content_index != 0 {
                                self.push_break_opportunity(content_index);
                            }
                            if content_end != self.run_text.len() {
                                self.push_break_opportunity(content_end);
                            }
                        }
                    }
                    InlineItem::SpanEnd => {
                        let popped = span_stack.pop().unwrap();
                        self.handle_span_end(current_style, &popped)?;
                        current_style = popped.parent_style;
                    }
                }
            }
            *end_item_index = current_item;

            self.flush_queued_text()?;

            debug_assert!(compute_break_opportunities || self.break_opportunities.is_empty());

            Ok(InitialShapingResult {
                shaped: self.shaped,
                break_opportunities: self.break_opportunities,
                text_leaf_items,
                bidi: self.bidi,
                font_feature_events: self.font_feature_events,
                grapheme_cluster_boundaries: self.grapheme_cluster_boundaries,
            })
        }
    }

    let run_text = &content.text_runs[run_index];
    let default_para_level = match inner_style.direction() {
        // TODO: `None` or `LTR_LEVEL` here? Picked `None` for now as that's the safest bet.
        computed::Direction::Ltr => None,
        computed::Direction::Rtl => Some(unicode_bidi::RTL_LEVEL),
    };
    ShapedItemBuilder {
        content,
        run_text,
        bidi: unicode_bidi::BidiInfo::new(run_text, default_para_level),
        grapheme_cluster_boundaries: {
            let mut result: Vec<usize> = GraphemeClusterSegmenter::new()
                .segment_str(run_text)
                .collect();
            // The segmenter always inserts `text.len()` as a grapheme cluster boundary
            // but we want this list to only include the start indices of graphemes.
            result.pop();
            result
        },
        lctx,
        writing_mode: content.root_style.writing_mode(),

        break_opportunities: Vec::new(),
        shaped: Vec::new(),
        span_state,
        shaping_buffer: ShapingBuffer::new(),
        queued_text_matcher: FontMatcher::new(
            util::rc_static!([]),
            text::FontStyle::default(),
            I26Dot6::ZERO,
            0,
        ),
        queued_text_segmenter: TextSegmenter::new(),
        font_feature_events: Vec::new(),
        queued_spacing: FixedL::ZERO,
        current_span_id: usize::MAX,
        total_content_bytes_added: 0,
    }
    .process_items(
        item_index,
        end_item_index,
        compute_break_opportunities,
        inner_style,
    )
}

struct BreakingContext<'l, 'a> {
    layout: &'a mut LayoutContext<'l>,
    available_space: FixedL,
    break_opportunities: &'a [usize],
    shaper: RunShaper<'a>,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // TODO: is this a problem here?
enum BreakOutcome<'p, 'c: 'p> {
    BreakSplit(ShapedItem<'c, FragmentStage<'p>>),
    BreakAfter,
    BreakBefore,
    None,
}

impl<'p, 'c: 'p> ShapedItem<'c, FragmentStage<'p>> {
    // TODO: Merge partial -> fragment conversion with line breaking?
    fn line_break(
        &mut self,
        current_width: &mut FixedL,
        ctx: &mut BreakingContext<'_, '_>,
    ) -> Result<BreakOutcome<'p, 'c>, InlineLayoutError> {
        let can_break_before = *current_width != FixedL::ZERO;
        *current_width += self.spacing.current_spacing_left;

        if *current_width > ctx.available_space {
            return Ok(BreakOutcome::BreakBefore);
        }

        match &mut self.kind {
            ShapedItemKind::Text(text) => text.line_break(
                &mut self.range,
                current_width,
                can_break_before,
                ctx,
                &mut self.spacing,
            ),
            ShapedItemKind::Ruby(_) | ShapedItemKind::Block(_) => {
                // TODO: Implement proper ruby line breaking
                //       It should only allow breaking between distinct base-annotation pairs.
                self.accumulate_content_width(current_width);
                *current_width += self.spacing.current_spacing_right;
                if *current_width > ctx.available_space {
                    Ok(BreakOutcome::BreakBefore)
                } else {
                    Ok(BreakOutcome::None)
                }
            }
        }
    }

    fn accumulate_content_width(&self, result: &mut FixedL) {
        match &self.kind {
            ShapedItemKind::Text(text) => {
                for (_, glyph) in text.glyphs.iter_glyphs_visual() {
                    *result += glyph.inline_advance(text.vertical_typesetting)
                }
            }
            ShapedItemKind::Ruby(ruby) => {
                for (base, annotation) in &ruby.base_annotation_pairs {
                    let mut base_width = FixedL::ZERO;
                    let mut annotation_width = FixedL::ZERO;

                    base.inner.accumulate_width(&mut base_width);
                    annotation.inner.accumulate_width(&mut annotation_width);

                    *result += base_width.max(annotation_width);
                }
            }
            ShapedItemKind::Block(block) => block.inner.accumulate_width(result),
        }
    }

    fn accumulate_width(&self, result: &mut FixedL) {
        *result += self.spacing.current_spacing_left;
        *result += self.spacing.current_spacing_right;
        self.accumulate_content_width(result);
    }

    fn forces_line_break_after(&self) -> bool {
        match &self.kind {
            ShapedItemKind::Text(text) => text.break_after,
            ShapedItemKind::Ruby(_) | ShapedItemKind::Block(_) => false,
        }
    }
}

impl TextItem {
    fn break_opportunity_to_range(&self, opportunity: usize) -> Range<usize> {
        let text = self.glyphs.text();
        let mut break_start_index = opportunity;
        let mut break_end_index = opportunity;
        while let Some(prev) = break_start_index.checked_sub(1) {
            if text.as_bytes()[prev] == b' ' {
                break_start_index = prev;
            } else {
                break;
            }
        }

        while text.as_bytes()[break_end_index] == b' ' {
            break_end_index += 1;
        }

        break_start_index..break_end_index
    }

    fn line_break<'a, 'c>(
        &mut self,
        range: &mut Range<usize>,
        current_width: &mut FixedL,
        can_break_before: bool,
        ctx: &mut BreakingContext<'_, '_>,
        spacing: &mut ShapedItemSpacing,
    ) -> Result<BreakOutcome<'a, 'c>, InlineLayoutError> {
        let initial_x = *current_width;
        let mut glyph_it = self.glyphs.iter_glyphs_logical().peekable();
        while let Some((_, glyph)) = glyph_it.next() {
            *current_width += glyph.inline_advance(self.vertical_typesetting);

            if glyph_it.peek().is_none() {
                *current_width += spacing.current_spacing_right;
            }

            if *current_width > ctx.available_space {
                // We want to also consider breaking within the current glyph so let's
                // start looking for break opportunities anywhere before the *next* glyph.
                let glyph_end = glyph.cluster + 1;
                let opportunities = &ctx.break_opportunities[..match ctx
                    .break_opportunities
                    .binary_search(&glyph_end)
                {
                    // TODO: This `+ 1` doesn't really make sense and is sort of a workaround
                    //       for whitespace handling shenanigans.
                    Ok(idx) => idx + 1,
                    Err(idx) => idx,
                }];

                // TODO: Also try slightly overflowing break points if these fail
                for &opportunity in opportunities
                    .iter()
                    .rev()
                    .take(3)
                    .take_while(|&&i| i > range.start)
                {
                    // FIXME: This is not how whitespace is supposed to be handled.
                    let break_range = self.break_opportunity_to_range(opportunity);
                    if let Some((broken, remaining)) = self.glyphs.break_around(
                        break_range,
                        ctx.available_space - initial_x,
                        &mut ctx.shaper,
                        self.font_matcher.iterator(),
                        self.vertical_typesetting,
                        ctx.layout,
                    )? {
                        drop(glyph_it);

                        let previous_end = range.end;
                        range.end = opportunity;
                        self.glyphs = broken;

                        return Ok(BreakOutcome::BreakSplit(ShapedItem {
                            range: opportunity..previous_end,
                            kind: ShapedItemKind::Text(TextItem {
                                font_matcher: self.font_matcher.clone(),
                                primary_font: self.primary_font.clone(),
                                glyphs: remaining,
                                break_after: self.break_after,
                                vertical_typesetting: self.vertical_typesetting,
                            }),
                            spacing: spacing.fragment_break(),
                        }));
                    }
                }

                // We failed to break inside the string and we're *not* the first item on the line,
                // so we can try breaking before the whole text run instead.
                if can_break_before {
                    return Ok(BreakOutcome::BreakBefore);
                }
            }
        }
        drop(glyph_it);

        if self.break_after {
            return Ok(BreakOutcome::BreakAfter);
        }

        Ok(BreakOutcome::None)
    }
}

fn layout_run_full<'a>(
    content: &'a InlineContent,
    initial_shaping_result: &InitialShapingResult<'a>,
    span_state: Vec<SpanState<'a>>,
    lctx: &mut LayoutContext,
    constraints: Vec2<LayoutConstraint>,
) -> Result<InlineContentFragment, InlineLayoutError> {
    fn split_on_leaves<'s, 'f>(
        range: Range<usize>,
        shaped: &TextItem,
        leaves: &[LeafItemRange<'s>],
        mut push_section: impl FnMut(
            &LeafItemRange<'s>,
            GlyphString,
            Range<usize>,
        ) -> Result<(), InlineLayoutError>,
    ) -> Result<(), InlineLayoutError> {
        let mut glyphs = shaped.glyphs.clone();

        let mut intersecting_leaves = slice_sorted_ranges_intersecting(leaves, range.clone());
        if intersecting_leaves.is_empty() {
            // FIXME: This is only necessary because empty lines currently create empty text items
            //        Instead explicit breaks should probably handled in another way
            return Ok(());
        } else if intersecting_leaves.len() == 1 {
            push_section(&intersecting_leaves[0], glyphs, range.clone())?;
            return Ok(());
        }

        if !glyphs.direction().is_reverse() {
            let mut start = range.start;
            while start != range.end {
                let (leaf, rest) = intersecting_leaves.split_first().unwrap();
                intersecting_leaves = rest;
                let end = if intersecting_leaves.is_empty() {
                    range.end
                } else {
                    leaf.range.end
                };

                if let Some(section_glyphs) = glyphs.split_off_visual_start(end) {
                    push_section(leaf, section_glyphs, start..end)?;
                }

                start = end;
            }
        } else {
            let mut end = range.end;
            while end != range.start {
                let (leaf, rest) = intersecting_leaves.split_last().unwrap();
                intersecting_leaves = rest;
                let start = if intersecting_leaves.is_empty() {
                    range.start
                } else {
                    leaf.range.start
                };

                if let Some(section_glyphs) = glyphs.split_off_visual_start(start) {
                    push_section(leaf, section_glyphs, start..end)?;
                }

                end = start;
            }
        };

        Ok(())
    }

    fn reorder<'p, 'c: 'p>(
        shaped: &mut [ShapedItem<'c, FragmentStage<'p>>],
        bidi: &unicode_bidi::BidiInfo,
        mut push_item: impl FnMut(
            &mut ShapedItem<'c, FragmentStage<'p>>,
        ) -> Result<(), InlineLayoutError>,
    ) -> Result<(), InlineLayoutError> {
        let line_range = {
            if let (Some(first), Some(last)) = (shaped.first(), shaped.last()) {
                first.range.start..last.range.end
            } else {
                // There's nothing to reorder, at most we'll run into indexing errors if
                // somehow there are paragraphs but not runs so we must bail here.
                return Ok(());
            }
        };

        if bidi.levels[line_range.clone()]
            .iter()
            .all(|level| level.is_ltr())
        {
            // The whole line only consists of LTR levels, hence no bidirectional reodering is
            // needed and we can skip all of this mess.
            for item in shaped {
                push_item(item)?;
            }

            return Ok(());
        }

        let mut visual_runs = Vec::new();
        for paragraph in &bidi.paragraphs {
            let start = line_range.start.max(paragraph.range.start);
            let end = line_range.end.min(paragraph.range.end);
            if start < end {
                let (_, mut paragraph_runs) = bidi.visual_runs(paragraph, start..end);
                visual_runs.append(&mut paragraph_runs);
            }
        }

        for range in visual_runs {
            let mut push_item_in_range =
                |item: &mut ShapedItem<'c, FragmentStage<'p>>| -> Result<(), InlineLayoutError> {
                    if range.start <= item.range.start && range.end >= item.range.end {
                        push_item(item)
                    } else if let ShapedItemKind::Text(text) = &item.kind {
                        assert!(
                            (range.start > item.range.start) ^ (range.end < item.range.end),
                            "bidi reordering attempted to partially reorder a text item on both sides"
                        );

                        // This case happens when bidi rule L1 reorders some whitespace inside a bidi
                        // level run.
                        // Since this is only supposed to affect whitespace glyphs, we don't reshape here
                        // assuming that the font is sane and does not ligate spaces. If it's not sane
                        // in this way, then true theoreteically correct line-breaking requires unbounded
                        // backtracking so it's not like we have much of a choice.

                        let mut tmp = TextItem {
                            font_matcher: text.font_matcher.clone(),
                            primary_font: text.primary_font.clone(),
                            glyphs: text.glyphs.clone(),
                            break_after: false,
                            vertical_typesetting: text.vertical_typesetting,
                        };
                        let split_range = if range.start > item.range.start {
                            tmp.glyphs.split_off_logical_end(range.start).map(|after| {
                                tmp.glyphs = after;
                                range.start..item.range.end
                            })
                        } else {
                            debug_assert!(range.end < item.range.end);
                            tmp.glyphs.split_off_logical_start(range.end).map(|before| {
                                tmp.glyphs = before;
                                item.range.start..range.end
                            })
                        };

                        if let Some(split_range) = split_range {
                            push_item(&mut ShapedItem {
                                range: split_range,
                                kind: ShapedItemKind::Text(tmp),
                                spacing: ShapedItemSpacing::MAX,
                            })
                        } else {
                            Ok(())
                        }
                    } else {
                        unreachable!(
                            "bidi reordering attempted to partially reorder a non-text item"
                        );
                    }
                };

            let level = bidi.levels[range.start];
            if level.is_ltr() {
                let start = match shaped.binary_search_by_key(&range.start, |r| r.range.start) {
                    Ok(i) => i,
                    Err(i) => i - 1,
                };

                for item in &mut shaped[start..] {
                    if item.range.start >= range.end {
                        break;
                    }

                    push_item_in_range(item)?;
                }
            } else {
                let end = match shaped.binary_search_by_key(&range.end, |r| r.range.end) {
                    Ok(i) => i + 1,
                    Err(i) => i + 1,
                };

                for item in shaped[..end].iter_mut().rev() {
                    if item.range.end <= range.start {
                        break;
                    }

                    push_item_in_range(item)?;
                }
            }
        }

        Ok(())
    }

    #[derive(Debug)]
    struct FragmentBuilder<'t, 'c> {
        result: InlineContentFragment,
        current_block_offset: FixedL,
        line_align: TextAlign,
        bidi: &'t unicode_bidi::BidiInfo<'c>,
        text_leaf_items: &'t [LeafItemRange<'c>],
        dpi: u32,
        content: &'c InlineContent,
        span_state: Vec<SpanState<'c>>,
    }

    #[derive(Debug)]
    struct InlineItemFragmentBuilder<'t, 'c> {
        output: &'t mut OffsetInlineItemFragmentVec,
        min_ruby_edge: &'t mut FixedL,
        max_ruby_edge: &'t mut FixedL,
        line_metrics: LineHeightMetrics,
        line_baseline: Baseline,
        line_baseline_block_offset: FixedL,
        current_inline_offset: FixedL,
        content: &'c InlineContent,
        writing_mode: WritingMode,
        dpi: u32,
    }

    #[derive(Debug, Clone, Copy)]
    struct LineHeightMetrics {
        max_ascender: FixedL,
        min_descender: FixedL,
    }

    #[derive(Debug, Clone, Copy)]
    enum LineHeight {
        Normal,
        Value(FixedL),
    }

    impl LineHeight {
        const ONE: Self = Self::Value(FixedL::ONE);
        const RUBY_ANNOTATION: Self = Self::ONE;
    }

    impl LineHeightMetrics {
        const ZERO: Self = LineHeightMetrics {
            max_ascender: FixedL::ZERO,
            min_descender: FixedL::ZERO,
        };

        fn height(&self) -> FixedL {
            self.max_ascender - self.min_descender
        }

        fn expand_to(&mut self, ascender: FixedL, descender: FixedL) {
            self.max_ascender = self.max_ascender.max(ascender);
            self.min_descender = self.min_descender.min(descender);
        }

        // https://drafts.csswg.org/css-inline/#inline-height
        fn process_item<'p, 'c: 'p>(
            &mut self,
            item: &ShapedItem<'c, FragmentStage<'p>>,
            line_height: LineHeight,
            dominant_baseline: Baseline,
            writing_mode: WritingMode,
        ) {
            match &item.kind {
                ShapedItemKind::Text(text) => match line_height {
                    LineHeight::Normal => {
                        let primary_metrics = text.primary_font.metrics();
                        let baseline = dominant_baseline
                            .metrics(primary_metrics, text.is_typographic_mode_vertical());
                        // NOTE: The `css-inline` spec doesn't say anything about writing modes here.
                        //       All browsers seem to agree on staying on horizontal metrics even
                        //       though it does feel kinda weird.
                        //       But I tried using vertical metrics and that has surprising behavior
                        //       with mixed upright and sideways text so maybe this is better.
                        let line_gap =
                            primary_metrics.horiz_height - baseline.ascender + baseline.descender;
                        let half_leading = (line_gap / 2).max(FixedL::ZERO);

                        self.expand_to(
                            baseline.ascender + half_leading,
                            baseline.descender - half_leading,
                        );

                        for font in text.glyphs.iter_fonts_logical() {
                            let glyph_metrics = font.metrics();
                            let glyph_baseline = dominant_baseline
                                .metrics(glyph_metrics, text.is_typographic_mode_vertical());
                            self.expand_to(
                                glyph_baseline.ascender + half_leading,
                                glyph_baseline.descender - half_leading,
                            );
                        }
                    }
                    LineHeight::Value(value) => {
                        let computed_font_size =
                            text.font_matcher.size() * text.font_matcher.dpi() as i32 / 72;
                        let baseline = dominant_baseline.metrics(
                            text.primary_font.metrics(),
                            text.is_typographic_mode_vertical(),
                        );
                        let half_leading = ((computed_font_size * value)
                            - (baseline.ascender - baseline.descender))
                            / 2;

                        self.expand_to(
                            baseline.ascender + half_leading,
                            baseline.descender - half_leading,
                        );
                    }
                },
                ShapedItemKind::Ruby(ruby) => {
                    for (base, _) in &ruby.base_annotation_pairs {
                        for item in &base.inner.items {
                            self.process_item(item, line_height, dominant_baseline, writing_mode);
                        }
                    }
                }
                ShapedItemKind::Block(BlockItem { inner: block, .. }) => {
                    let ascender = block.dominant_baseline_block_offset;
                    let descender = ascender - block.fragment.fbox.block_size(writing_mode);
                    self.expand_to(ascender, descender);
                }
            }
        }

        fn as_baseline_metrics(&self) -> BaselineMetrics {
            BaselineMetrics {
                ascender: self.max_ascender,
                descender: self.min_descender,
            }
        }

        fn block_start_offset(&self, writing_mode: WritingMode) -> FixedL {
            self.as_baseline_metrics().block_start_offset(writing_mode)
        }

        fn block_end_offset(&self, writing_mode: WritingMode) -> FixedL {
            self.as_baseline_metrics().block_end_offset(writing_mode)
        }
    }

    impl<'o, 'p, 'c: 'p> InlineItemFragmentBuilder<'o, 'c> {
        fn child_builder<'o2>(
            &mut self,
            output: &'o2 mut OffsetInlineItemFragmentVec,
            min_ruby_y: &'o2 mut FixedL,
            max_ruby_y: &'o2 mut FixedL,
            line_metrics: LineHeightMetrics,
            top_y: FixedL,
            current_inline_offset: FixedL,
        ) -> InlineItemFragmentBuilder<'o2, 'c> {
            InlineItemFragmentBuilder {
                output,
                min_ruby_edge: min_ruby_y,
                max_ruby_edge: max_ruby_y,
                line_metrics,
                line_baseline_block_offset: top_y,
                current_inline_offset,
                dpi: self.dpi,
                writing_mode: self.writing_mode,
                line_baseline: self.line_baseline,
                content: self.content,
            }
        }

        fn inline_box_sizing_metrics(
            &self,
            style: &ComputedStyle,
            font_metrics: &FontMetrics,
        ) -> BaselineMetrics {
            // https://drafts.csswg.org/css-inline/#line-fill
            match style.inline_sizing() {
                InlineSizing::Normal => self.line_baseline.metrics(font_metrics, false),
                InlineSizing::Stretch => self.line_metrics.as_baseline_metrics(),
            }
        }

        fn rebuild_leaf_branch(
            &mut self,
            mut span_id: usize,
            leaf_inline_size: FixedL,
            leaf_block_offset: FixedL,
            leaf: util::rc::Rc<InlineItemFragment>,
            content_len: usize,
            // NOTE: I tried putting this in `InlineItemFragmentBuilder` but lifetime hell
            //       ensued. Maybe try taming that at some point in the future.
            span_state: &mut [SpanState],
        ) {
            let mut result = leaf;

            let mut current_inner_inline_size = leaf_inline_size;
            let mut current_block_offset = leaf_block_offset;

            // NOTE: can't use `SpanState::walk_up` because of `result` moving shenanigans
            while span_id != usize::MAX {
                let state = &mut span_state[span_id];
                let font_metrics = state.primary_font.metrics();

                let sizing_metrics = self.inline_box_sizing_metrics(state.style, font_metrics);
                let top_block_offset = self.line_baseline_block_offset
                    - sizing_metrics.block_start_offset(self.writing_mode);

                let mut part = BoxFragmentationPart::VERTICAL_FULL;
                if !state.seen_first {
                    part |= BoxFragmentationPart::HORIZONTAL_FIRST;
                    state.seen_first = true;
                }
                state.remaining_line_content_bytes -= content_len as u32;
                if state.remaining_content_bytes == 0 && state.remaining_line_content_bytes == 0 {
                    part |= BoxFragmentationPart::HORIZONTAL_LAST;
                }

                if self.writing_mode.is_vertical() {
                    part = part.swap_axes();
                }

                let fbox = FragmentBox {
                    content_size: Vec2LW::new(sizing_metrics.height(), current_inner_inline_size)
                        .to_physical(self.writing_mode),
                    padding: EdgeExtents::padding_fragmented(part, state.style, self.dpi),
                    margin: EdgeExtents::margins_auto_to_zero_fragmented(
                        part,
                        state.style,
                        self.dpi,
                    ),
                };
                current_inner_inline_size = fbox.inline_size(self.writing_mode);
                result = util::rc::Rc::new(InlineItemFragment::Span(SpanFragment {
                    content: vec![(
                        Vec2LW::new(current_block_offset - top_block_offset, FixedL::ZERO)
                            .to_physical(self.writing_mode),
                        result,
                    )],
                    fbox,
                    style: state.style.clone(),
                    primary_font: state.primary_font.clone(),
                }));
                current_block_offset =
                    top_block_offset - fbox.content_offset().block(self.writing_mode);
                span_id = state.parent;
            }

            self.output.push((
                Vec2LW::new(current_block_offset, self.current_inline_offset)
                    .to_physical(self.writing_mode),
                result,
            ));
            self.current_inline_offset += current_inner_inline_size;
        }

        fn push_reordered(
            &mut self,
            item: &mut ShapedItem<'c, FragmentStage<'p>>,
            text_leaf_items: &[LeafItemRange<'c>],
            span_state: &mut [SpanState],
        ) -> Result<(), InlineLayoutError> {
            match &mut item.kind {
                ShapedItemKind::Text(text) => split_on_leaves(
                    item.range.clone(),
                    text,
                    text_leaf_items,
                    |leaf, glyphs, range| {
                        let inner_inline_size: FixedL = glyphs
                            .iter_glyphs_visual()
                            .map(|(_, g)| g.inline_advance(text.vertical_typesetting))
                            .sum();

                        let fragment = TextFragment {
                            style: leaf.style.clone(),
                            glyphs,
                            inline_size: inner_inline_size,
                            vertical_typesetting: text.vertical_typesetting,
                            align_to: self.line_baseline,
                        };

                        self.rebuild_leaf_branch(
                            leaf.span_id,
                            inner_inline_size,
                            self.line_baseline_block_offset,
                            InlineItemFragment::Text(fragment).into(),
                            range.len(),
                            span_state,
                        );

                        Ok(())
                    },
                ),
                ShapedItemKind::Ruby(ruby) => {
                    let mut result = RubyFragment {
                        // TODO: What box should a ruby container fragment have?
                        //       For now we'll just leave it zero-sized.
                        fbox: FragmentBox::ZERO,
                        style: ruby.style.clone(),
                        content: Vec::new(),
                    };

                    let mut ruby_current_inline_offset = FixedL::ZERO;
                    for (base, annotation) in &mut ruby.base_annotation_pairs {
                        let mut base_inner_inline_size = FixedL::ZERO;
                        let mut base_metrics = LineHeightMetrics::ZERO;
                        let mut annotation_inner_inline_size = FixedL::ZERO;
                        let mut annotation_metrics = LineHeightMetrics::ZERO;
                        for item in &base.inner.items {
                            item.accumulate_width(&mut base_inner_inline_size);
                            base_metrics.process_item(
                                item,
                                LineHeight::ONE,
                                self.line_baseline,
                                self.writing_mode,
                            );
                        }
                        for item in &annotation.inner.items {
                            item.accumulate_width(&mut annotation_inner_inline_size);
                            annotation_metrics.process_item(
                                item,
                                LineHeight::RUBY_ANNOTATION,
                                self.line_baseline,
                                self.writing_mode,
                            );
                        }

                        let base_font_metrics = base.primary_font.metrics();
                        let base_sizing_metrics =
                            self.inline_box_sizing_metrics(base.style, base_font_metrics);
                        let mut base_inner_block_offset =
                            base_sizing_metrics.block_start_offset(self.writing_mode);
                        let mut base_block_offset = -base_inner_block_offset;
                        let mut base_block_size = base_sizing_metrics.height();

                        let annotation_block_size = annotation_metrics.height();
                        let annotation_block_offset = if !self.writing_mode.is_line_flipped() {
                            -base_metrics.max_ascender - annotation_block_size
                        } else {
                            base_metrics.max_ascender
                        };
                        let signed_half_padding =
                            (annotation_inner_inline_size - base_inner_inline_size) / 2;
                        let base_half_padding = signed_half_padding.max(FixedL::ZERO);
                        let annotation_half_padding = (-signed_half_padding).max(FixedL::ZERO);
                        let ruby_inner_inline_size =
                            base_inner_inline_size.max(annotation_inner_inline_size);

                        // HACK: This is to make ruby base and annotation backgrounds not overlap.
                        //       The correct solution is block layout (#111) or a subrandr-specific
                        //       style property.
                        let background_overlap = (annotation_block_offset - base_block_offset
                            + annotation_block_size)
                            .max(FixedL::ZERO);
                        base_block_offset += background_overlap;
                        base_inner_block_offset -= background_overlap;
                        base_block_size -= background_overlap;

                        if !self.writing_mode.is_line_flipped() {
                            *self.min_ruby_edge =
                                (*self.min_ruby_edge).min(annotation_block_offset);
                        } else {
                            *self.max_ruby_edge = (*self.max_ruby_edge)
                                .max(annotation_block_offset + annotation_block_size);
                        }

                        // FIXME: Apparently ruby internal boxes are not supposed to use
                        //        inline-sizing sizing. Now this makes sense with the ruby
                        //        annotation box because it creates/is a new line box and
                        //        should logically obey line box sizing rules.
                        //        However I'm not certain what this means for ruby base
                        //        boxes? Should they just fit their contents?
                        let mut base_fragment = RubyBaseFragment {
                            fbox: FragmentBox {
                                content_size: Vec2LW::new(base_block_size, ruby_inner_inline_size)
                                    .to_physical(self.writing_mode),
                                padding: EdgeExtents::padding(base.style, self.dpi),
                                margin: EdgeExtents::margins_auto_to_zero(base.style, self.dpi),
                            },
                            style: base.style.clone(),
                            primary_font: base.primary_font.clone(),
                            children: Vec::new(),
                        };

                        self.child_builder(
                            &mut base_fragment.children,
                            // TODO: ruby nested in base
                            &mut { FixedL::ZERO },
                            &mut { FixedL::ZERO },
                            self.line_metrics,
                            base_inner_block_offset,
                            base_half_padding,
                        )
                        .reorder_and_append(
                            &mut base.inner.items,
                            &base.inner.initial.bidi,
                            &base.inner.initial.text_leaf_items,
                            span_state,
                        )?;

                        let annotation_line_baseline_offset =
                            annotation_metrics.block_start_offset(self.writing_mode);
                        let mut annotation_fragment = RubyAnnotationFragment {
                            fbox: FragmentBox {
                                content_size: Vec2LW::new(
                                    annotation_block_size,
                                    ruby_inner_inline_size,
                                )
                                .to_physical(self.writing_mode),
                                padding: EdgeExtents::padding(annotation.style, self.dpi),
                                margin: EdgeExtents::margins_auto_to_zero(
                                    annotation.style,
                                    self.dpi,
                                ),
                            },
                            style: annotation.style.clone(),
                            primary_font: annotation.primary_font.clone(),
                            baseline_block_offset: annotation_line_baseline_offset,
                            children: Vec::new(),
                        };

                        self.child_builder(
                            &mut annotation_fragment.children,
                            // TODO: ruby nested in annotation
                            &mut { FixedL::ZERO },
                            &mut { FixedL::ZERO },
                            annotation_metrics,
                            annotation_line_baseline_offset,
                            annotation_half_padding,
                        )
                        .reorder_and_append(
                            &mut annotation.inner.items,
                            &annotation.inner.initial.bidi,
                            &annotation.inner.initial.text_leaf_items,
                            span_state,
                        )?;

                        let base_inline_size = base_fragment.fbox.inline_size(self.writing_mode);
                        result.content.push((
                            Vec2LW::new(
                                self.line_baseline_block_offset + base_block_offset,
                                ruby_current_inline_offset,
                            )
                            .to_physical(self.writing_mode),
                            base_fragment,
                            Vec2LW::new(
                                self.line_baseline_block_offset + annotation_block_offset,
                                ruby_current_inline_offset,
                            )
                            .to_physical(self.writing_mode),
                            annotation_fragment,
                        ));
                        ruby_current_inline_offset += base_inline_size;
                    }

                    self.rebuild_leaf_branch(
                        ruby.span_id,
                        ruby_current_inline_offset,
                        FixedL::ZERO,
                        InlineItemFragment::Ruby(result).into(),
                        OBJECT_REPLACEMENT_LENGTH,
                        span_state,
                    );

                    Ok(())
                }
                &mut ShapedItemKind::Block(BlockItem {
                    span_id,
                    inner: ref mut block,
                }) => {
                    let inner_inline_size = block.fragment.fbox.inline_size(self.writing_mode);
                    self.rebuild_leaf_branch(
                        span_id,
                        inner_inline_size,
                        self.line_baseline_block_offset - block.dominant_baseline_block_offset,
                        InlineItemFragment::Block(std::mem::replace(
                            &mut block.fragment,
                            BlockContainerFragment::EMPTY,
                        ))
                        .into(),
                        OBJECT_REPLACEMENT_LENGTH,
                        span_state,
                    );

                    Ok(())
                }
            }
        }

        fn reorder_and_append(
            &mut self,
            shaped: &mut [ShapedItem<'c, FragmentStage<'p>>],
            bidi: &unicode_bidi::BidiInfo<'c>,
            text_leaf_items: &[LeafItemRange<'c>],
            span_state: &mut [SpanState],
        ) -> Result<(), InlineLayoutError> {
            reorder(shaped, bidi, |item| {
                self.push_reordered(item, text_leaf_items, span_state)
            })?;

            Ok(())
        }
    }

    impl<'t, 'p, 'c: 'p> FragmentBuilder<'t, 'c> {
        fn split_on_leaves_for_fragmentation(
            item: &ShapedItem<'c, FragmentStage<'p>>,
            leaves: &[LeafItemRange],
            mut on_leaf: impl FnMut(usize, Range<usize>),
        ) {
            match &item.kind {
                ShapedItemKind::Text(_) => {
                    let mut intersecting_leaves =
                        slice_sorted_ranges_intersecting(leaves, item.range.clone());
                    let mut start = item.range.start;
                    while start != item.range.end {
                        let (leaf, rest) = intersecting_leaves.split_first().unwrap();
                        intersecting_leaves = rest;
                        if intersecting_leaves.is_empty() {
                            on_leaf(leaf.span_id, start..item.range.end);
                            break;
                        } else {
                            on_leaf(leaf.span_id, start..leaf.range.end);
                            start = leaf.range.end;
                        };
                    }
                }
                ShapedItemKind::Ruby(ruby) => on_leaf(ruby.span_id, item.range.clone()),
                &ShapedItemKind::Block(BlockItem { span_id, .. }) => {
                    on_leaf(span_id, item.range.clone())
                }
            }
        }

        fn update_line_fragmentation_state_pre(
            &mut self,
            shaped_item: &ShapedItem<'c, FragmentStage<'p>>,
            leaves: &[LeafItemRange],
        ) {
            Self::split_on_leaves_for_fragmentation(shaped_item, leaves, |span_id, range| {
                let range_len = range.len();

                SpanState::walk_up(&mut self.span_state, span_id, |state| {
                    state.remaining_content_bytes -= range_len as u32;
                    state.remaining_line_content_bytes += range_len as u32;
                });
            });

            if let ShapedItemKind::Ruby(ruby) = &shaped_item.kind {
                for (base, annotation) in &ruby.base_annotation_pairs {
                    for item in &base.inner.items {
                        self.update_line_fragmentation_state_pre(
                            item,
                            &base.inner.initial.text_leaf_items,
                        );
                    }

                    for item in &annotation.inner.items {
                        self.update_line_fragmentation_state_pre(
                            item,
                            &annotation.inner.initial.text_leaf_items,
                        );
                    }
                }
            }
        }

        fn push_line(
            &mut self,
            shaped: &mut [ShapedItem<'c, FragmentStage<'p>>],
        ) -> Result<(), InlineLayoutError> {
            let writing_mode = self.content.root_style.writing_mode();
            let line_baseline = writing_mode.auto_dominant_baseline();
            let mut line_inline_size = FixedL::ZERO;
            let mut line_metrics = LineHeightMetrics::ZERO;
            for item in &*shaped {
                item.accumulate_width(&mut line_inline_size);
                line_metrics.process_item(item, LineHeight::Normal, line_baseline, writing_mode);
                self.update_line_fragmentation_state_pre(item, self.text_leaf_items);
            }
            let line_block_size = line_metrics.height();

            let mut line_box = LineBoxFragment {
                fbox: FragmentBox::new_content_only(
                    Vec2LW::new(line_block_size, line_inline_size).to_physical(writing_mode),
                ),
                dominant_baseline_offset: line_metrics.block_start_offset(writing_mode),
                children: Vec::new(),
            };

            let mut min_ruby_edge = FixedL::ZERO;
            let mut max_ruby_edge = FixedL::ZERO;
            {
                InlineItemFragmentBuilder {
                    output: &mut line_box.children,
                    min_ruby_edge: &mut min_ruby_edge,
                    max_ruby_edge: &mut max_ruby_edge,
                    line_metrics,
                    line_baseline_block_offset: line_metrics.block_start_offset(writing_mode),
                    current_inline_offset: FixedL::ZERO,
                    content: self.content,
                    writing_mode,
                    line_baseline,
                    dpi: self.dpi,
                }
                .reorder_and_append(
                    shaped,
                    self.bidi,
                    self.text_leaf_items,
                    &mut self.span_state,
                )?;
            }

            // Make sure that our "fragile" byte coverage calculations were correct.
            // `finish()` also makes sure the total content byte coverage was all
            // accounted for.
            for item in &*shaped {
                Self::split_on_leaves_for_fragmentation(
                    item,
                    self.text_leaf_items,
                    |span_id, _| {
                        SpanState::walk_up(&mut self.span_state, span_id, |state| {
                            // FIXME: This **can** happen because `split_on_leaves` doesn't push sections
                            //        without any glyphs, basically this is an issue only in extreme
                            //        edge cases and falls into the category of "empty span" issues.
                            // debug_assert_eq!(state.remaining_line_content_bytes, 0);
                            state.remaining_line_content_bytes = 0;
                        });
                    },
                );
            }

            let aligning_inline_offset = match self.line_align {
                TextAlign::Left => I26Dot6::ZERO,
                TextAlign::Center => -line_inline_size / 2,
                TextAlign::Right => -line_inline_size,
            };

            min_ruby_edge += line_metrics.block_start_offset(writing_mode);
            max_ruby_edge += line_metrics.block_end_offset(writing_mode);

            let ruby_leading_start = (-min_ruby_edge).max(FixedL::ZERO);
            let ruby_leading_end = max_ruby_edge.max(FixedL::ZERO);

            let max_inline_size = self.result.fbox.content_size.inline_mut(writing_mode);
            *max_inline_size = (*max_inline_size).max(line_inline_size);

            self.current_block_offset += ruby_leading_start;
            let mut offset = Vec2LW::new(self.current_block_offset, aligning_inline_offset)
                .to_physical(writing_mode);
            self.current_block_offset += line_block_size;
            self.current_block_offset += ruby_leading_end;
            if writing_mode.is_block_reversed() {
                *offset.block_mut(writing_mode) = -self.current_block_offset;
            }
            self.result.lines.push((offset, line_box.into()));

            Ok(())
        }

        fn finish(self) -> InlineContentFragment {
            let mut fragment = self.result;

            #[cfg(debug_assertions)]
            for span_state in self.span_state {
                assert_eq!(
                    span_state.remaining_content_bytes, 0,
                    "a span's content byte counter wasn't exhausted"
                );
            }

            let writing_mode = self.content.root_style.writing_mode();
            let mut min_inline = FixedL::ZERO;
            for (offset, _) in &fragment.lines {
                min_inline = min_inline.min(offset.inline(writing_mode));
            }
            for (offset, _) in &mut fragment.lines {
                *offset.inline_mut(writing_mode) -= min_inline;
            }

            if writing_mode.is_block_reversed() {
                for (offset, _) in &mut fragment.lines {
                    *offset.block_mut(writing_mode) += self.current_block_offset;
                }
            }

            *fragment.fbox.content_size.block_mut(writing_mode) = self.current_block_offset;

            fragment
        }
    }

    let writing_mode = content.root_style.writing_mode();
    let FragmentShapingResult {
        initial:
            InitialShapingResult {
                ref break_opportunities,
                ref text_leaf_items,
                ref bidi,
                ref font_feature_events,
                ref grapheme_cluster_boundaries,
                ..
            },
        mut items,
    } = initial_shaping_result.to_fragment_result(
        lctx,
        constraints,
        writing_mode,
        writing_mode.auto_dominant_baseline(),
    )?;

    let root_primary_font = primary_font_from_style(&content.root_style, lctx)?;
    let mut builder = FragmentBuilder {
        current_block_offset: FixedL::ZERO,
        result: InlineContentFragment {
            style: content.root_style.clone(),
            line_baselines: {
                let alphabetic_centered_set =
                    BoxBaselineSet::new(root_primary_font.metrics(), writing_mode);
                alphabetic_centered_set
                    .offset(-alphabetic_centered_set.get(writing_mode.auto_dominant_baseline()))
            },
            primary_font_metrics: *root_primary_font.metrics(),
            ..InlineContentFragment::EMPTY
        },
        line_align: content.root_style.text_align(),
        bidi,
        text_leaf_items,
        dpi: lctx.dpi,
        content,
        span_state,
    };

    let mut items = &mut items[..];
    let available_space = match constraints.inline(writing_mode) {
        LayoutConstraint::Fixed(fixed) => fixed,
        LayoutConstraint::MaxContent => FixedL::MAX,
    };
    if available_space != FixedL::MAX && !break_opportunities.is_empty() {
        let mut breaking_context = BreakingContext {
            layout: lctx,
            available_space,
            break_opportunities,
            shaper: RunShaper {
                buffer: &mut text::ShapingBuffer::new(),
                font_feature_events,
                grapheme_cluster_boundaries,
            },
        };

        'break_loop: loop {
            let mut current_width = FixedL::ZERO;
            'item_loop: for mut i in 0..items.len() {
                let item = &mut items[i];
                let remaining = match item.line_break(&mut current_width, &mut breaking_context)? {
                    BreakOutcome::BreakSplit(item) => Some(item),
                    BreakOutcome::BreakAfter => None,
                    BreakOutcome::BreakBefore => {
                        i = i.saturating_sub(1);
                        None
                    }
                    BreakOutcome::None => continue 'item_loop,
                };

                builder.push_line(&mut items[..=i])?;

                if let Some(remaining) = remaining {
                    items = &mut items[i..];
                    *items.first_mut().unwrap() = remaining;
                } else {
                    items = &mut items[i + 1..];
                }

                continue 'break_loop;
            }

            if !items.is_empty() {
                builder.push_line(items)?;
            }
            break;
        }
    } else {
        'break_loop: loop {
            for i in 0..items.len() {
                if items[i].forces_line_break_after() {
                    builder.push_line(&mut items[..=i])?;
                    items = &mut items[i + 1..];
                    continue 'break_loop;
                }
            }

            if !items.is_empty() {
                builder.push_line(items)?;
            }
            break;
        }
    }

    Ok(builder.finish())
}

pub struct PartialInline<'a> {
    content: &'a InlineContent,
    span_state: Vec<SpanState<'a>>,
    initial_shaping_result: InitialShapingResult<'a>,
}

pub fn shape<'l, 'b, 'c>(
    lctx: &'b mut LayoutContext<'l>,
    content: &'c InlineContent,
) -> Result<PartialInline<'c>, InlineLayoutError> {
    if content.text_runs.is_empty() {
        return Ok(PartialInline {
            content,
            span_state: Vec::new(),
            initial_shaping_result: InitialShapingResult::empty(),
        });
    }

    let mut span_state = Vec::new();
    let initial_shaping_result = shape_run_initial(
        content,
        0,
        0,
        &mut content.items.len(),
        lctx,
        true,
        &mut span_state,
        &content.root_style,
    )?;

    Ok(PartialInline {
        content,
        span_state,
        initial_shaping_result,
    })
}

impl PartialInline<'_> {
    pub(super) fn root_style(&self) -> &ComputedStyle {
        &self.content.root_style
    }

    fn max_inline_size(
        &self,
        lctx: &mut LayoutContext,
        block_constraint: LayoutConstraint,
    ) -> Result<FixedL, InlineLayoutError> {
        // TODO: This could actually be avoided (and it was avoided before but removed for simplicity)
        //       by just measuring the partial items directly.
        let writing_mode = self.content.root_style.writing_mode();
        let items = self
            .initial_shaping_result
            .to_fragment_result(
                lctx,
                Vec2W::new(block_constraint, LayoutConstraint::MaxContent)
                    .to_physical(writing_mode),
                writing_mode,
                writing_mode.auto_dominant_baseline(),
            )?
            .items;

        let mut max = FixedL::ZERO;
        let mut current = FixedL::ZERO;
        for item in items {
            item.accumulate_width(&mut current);
            if item.forces_line_break_after() {
                max = max.max(current);
                current = FixedL::ZERO;
            }
        }
        Ok(max.max(current))
    }

    pub(crate) fn measure(
        &self,
        lctx: &mut LayoutContext<'_>,
        constraints: Vec2<LayoutConstraint>,
        axes: Axes,
    ) -> Result<Vec2L, InlineLayoutError> {
        let writing_mode = self.content.root_style.writing_mode();
        if constraints.inline(writing_mode) == LayoutConstraint::MaxContent
            && !axes.block(writing_mode)
        {
            return Ok(Vec2LW::new(
                FixedL::ZERO,
                self.max_inline_size(lctx, constraints.block(writing_mode))?,
            )
            .to_physical(writing_mode));
        }

        layout_run_full(
            self.content,
            &self.initial_shaping_result,
            self.span_state.clone(),
            lctx,
            constraints,
        )
        .map(|x| x.fbox.size_for_layout())
    }

    pub(super) fn layout<'b, 'l>(
        &self,
        lctx: &'b mut LayoutContext<'l>,
        constraints: Vec2<LayoutConstraint>,
    ) -> Result<InlineContentFragment, InlineLayoutError> {
        layout_run_full(
            self.content,
            &self.initial_shaping_result,
            self.span_state.clone(),
            lctx,
            constraints,
        )
    }
}

pub fn layout<'l, 'b, 'c>(
    lctx: &'b mut LayoutContext<'l>,
    content: &'c InlineContent,
    initial_containing_block_size: Vec2L,
) -> Result<InlineContentFragment, InlineLayoutError> {
    lctx.initial_containing_block_size = initial_containing_block_size;

    let constraints = Vec2::new(
        LayoutConstraint::Fixed(initial_containing_block_size.x),
        LayoutConstraint::Fixed(initial_containing_block_size.y),
    );
    shape(lctx, content).and_then(|s| s.layout(lctx, constraints))
}

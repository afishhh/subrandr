use std::{
    num::NonZero,
    ops::{Deref, DerefMut, Range},
    rc::Rc,
};

use icu_segmenter::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};
use thiserror::Error;
use util::math::{I16Dot16, I26Dot6, Point2, Rect2, Vec2};

use crate::{
    layout::{FixedL, InlineLayoutError, LayoutContext},
    log::{error, log_once_state},
    style::{
        self,
        types::{FontSlant, HorizontalAlignment},
        CascadingStyleMap, StyleMap,
    },
    text::{self, FontArena, FontDb, FontMatcher, GlyphString, TextMetrics},
    Subrandr,
};

use super::{Direction, ShapingBuffer};

// TODO: Bidi Mirroring (https://www.unicode.org/reports/tr9/#L4)
//       unless harfbuzz already does that I don't know

// I don't care anymore let's just inline the code...
macro_rules! inline_runs_get_or_insert_text_nll_case_3 {
    ($runs: ident, $default_range: expr) => {
        if let Some(Run::Text(run)) = $runs.0.last_mut() {
            run
        } else {
            $runs.0.push(Run::Text(TextRun {
                range: $default_range,
                segments: Vec::new(),
            }));
            match $runs.0.last_mut().unwrap() {
                Run::Text(run) => run,
                _ => unreachable!(),
            }
        }
    };
}

pub struct InlineContent {
    main_text_content: Rc<str>,
    segments: Vec<InlineItem>,
}

pub struct InlineContentBuilder {
    result_text: String,
    segments: Vec<InlineItem>,
}

impl InlineContentBuilder {
    pub fn new() -> Self {
        Self {
            result_text: String::new(),
            segments: Vec::new(),
        }
    }

    pub fn as_span_builder(&mut self) -> InlineSpanBuilder {
        InlineSpanBuilder {
            parent: self,
            span_index: usize::MAX,
            length: 0,
        }
    }

    pub fn build(&mut self) -> InlineContent {
        InlineContent {
            main_text_content: self.result_text.as_str().into(),
            segments: std::mem::take(&mut self.segments),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum UnicodeBidi {
    Normal,
    Embed,
    // ruby boxes have this by default
    // NOTE: ruby boxes have special "computes to" clauses in the spec - this has to
    //       be taken into account in miniweb.
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

impl UnicodeBidi {
    // https://www.w3.org/TR/css-writing-modes-4/#bidi-isolate
    fn control_characters(self, is_rtl: bool) -> Option<(&'static str, &'static str)> {
        match (self, is_rtl) {
            (UnicodeBidi::Normal, _) => None,
            (UnicodeBidi::Embed, false) => Some(("\u{202A}", "\u{202C}")),
            (UnicodeBidi::Isolate, false) => Some(("\u{2066}", "\u{2069}")),
            (UnicodeBidi::BidiOverride, false) => Some(("\u{202D}", "\u{202C}")),
            (UnicodeBidi::IsolateOverride, false) => Some(("\u{2068}\u{202D}", "\u{202C}\u{2069}")),
            (UnicodeBidi::Embed, true) => Some(("\u{202B}", "\u{202C}")),
            (UnicodeBidi::Isolate, true) => Some(("\u{2067}", "\u{2069}")),
            (UnicodeBidi::BidiOverride, true) => Some(("\u{202E}", "\u{202C}")),
            (UnicodeBidi::IsolateOverride, true) => Some(("\u{2068}\u{202E}", "\u{202C}\u{2069}")),
            (UnicodeBidi::Plaintext, _) => Some(("\u{2068}", "\u{2069}")),
        }
    }
}

pub struct InlineSpanBuilder<'a> {
    parent: &'a mut InlineContentBuilder,
    span_index: usize,
    length: usize,
}

impl<'a> InlineSpanBuilder<'a> {
    fn span_mut(&mut self) -> &mut InlineSpan {
        match &mut self.parent.segments[self.span_index] {
            InlineItem::Span(span) => span,
            _ => unreachable!(),
        }
    }

    pub fn push_text(&mut self, content: &str) {
        let start = self.parent.result_text.len();
        self.parent.result_text.push_str(content);
        self.parent.segments.push(InlineItem::Text(InlineText {
            content_range: start..self.parent.result_text.len(),
        }));
        self.length += 1;
    }

    fn push_span_with(&mut self, style: StyleMap, kind: InlineSpanKind) -> InlineSpanBuilder<'_> {
        let index = self.parent.segments.len();
        self.parent.segments.push(InlineItem::Span(InlineSpan {
            style,
            length: 0,
            kind,
        }));
        self.length += 1;
        InlineSpanBuilder {
            parent: self.parent,
            span_index: index,
            length: self.length,
        }
    }

    pub fn push_span(&mut self, style: StyleMap) -> InlineSpanBuilder<'_> {
        self.push_span_with(style, InlineSpanKind::Span)
    }

    pub fn push_ruby(&mut self, style: StyleMap) -> InlineRubyBuilder<'_> {
        InlineRubyBuilder(self.push_span_with(
            style,
            InlineSpanKind::Ruby(RubySpan {
                annotations: Vec::new(),
            }),
        ))
    }
}

impl<'a> Drop for InlineSpanBuilder<'a> {
    fn drop(&mut self) {
        if self.span_index == usize::MAX {
            return;
        }

        self.span_mut().length = self.length;
    }
}

pub struct InlineRubyBuilder<'a>(InlineSpanBuilder<'a>);

impl<'a> InlineRubyBuilder<'a> {
    pub fn push_annotation(
        &mut self,
        level: RubyLevel,
        bases: Range<usize>,
        content: InlineContent,
    ) {
        match self.span_mut().kind {
            InlineSpanKind::Span => unreachable!(),
            InlineSpanKind::Ruby(ref mut ruby) => {
                ruby.annotations.push(RubyAnnotation2 {
                    level,
                    bases,
                    content,
                });
            }
        }
    }
}

impl<'a> Deref for InlineRubyBuilder<'a> {
    type Target = InlineSpanBuilder<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for InlineRubyBuilder<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub enum InlineItem {
    Span(InlineSpan),
    Text(InlineText),
}

pub struct InlineSpan {
    style: StyleMap,
    length: usize,
    kind: InlineSpanKind,
}

pub enum InlineSpanKind {
    Span,
    Ruby(RubySpan),
}

// The direct children of this span are the ruby bases while the
// annotations are stored in the contained vector.
pub struct RubySpan {
    annotations: Vec<RubyAnnotation2>,
}

pub struct RubyAnnotation2 {
    level: RubyLevel,
    bases: Range<usize>,
    content: InlineContent,
}

pub struct InlineText {
    content_range: Range<usize>,
}

// tmp pub
pub(crate) struct FontContext<'l, 'a, 'f> {
    pub layout: LayoutContext<'l, 'a>,
    pub font_arena: &'f FontArena,
    pub shaping_buffer: ShapingBuffer,
}

// tmp pub
pub fn content_to_runs<'a, 'f>(
    content: &'a InlineContent,
    fctx: &mut FontContext<'_, '_, 'f>,
    base_style: CascadingStyleMap,
) -> Result<(InlineRuns<'a, 'f>, unicode_bidi::BidiInfo<'a>), InlineLayoutError> {
    let bidi = unicode_bidi::BidiInfo::new(&content.main_text_content, None);
    let mut result = InlineRuns::new();

    fn visit<'a, 'f>(
        result: &mut InlineRuns<'a, 'f>,
        bidi: &unicode_bidi::BidiInfo,
        content: &'a InlineContent,
        fctx: &mut FontContext<'_, '_, 'f>,
        style: CascadingStyleMap,
        current: &mut usize,
        limit: usize,
    ) -> Result<(), InlineLayoutError> {
        for _ in 0..limit {
            let item = content.segments.get(*current);
            *current += 1;
            match item {
                Some(InlineItem::Span(span)) => {
                    let new_style = style.push(&span.style);
                    match &span.kind {
                        InlineSpanKind::Span => {
                            visit(result, bidi, content, fctx, new_style, current, span.length)?;
                        }
                        InlineSpanKind::Ruby(ruby) => {
                            let mut run = RubyRun {
                                range: usize::MAX..usize::MAX,
                                bases: Vec::new(),
                                annotations: Vec::new(),
                            };
                            for _ in 0..span.length {
                                let mut base = InlineRuns::new();
                                visit(
                                    &mut base,
                                    bidi,
                                    content,
                                    fctx,
                                    new_style.clone(),
                                    current,
                                    1,
                                )?;
                                run.bases.push(base);
                            }
                            if let (Some(first), Some(last)) = (
                                run.bases.iter().find_map(|r| r.0.first()),
                                run.bases.iter().rev().find_map(|r| r.0.last()),
                            ) {
                                run.range = first.byte_range().start..last.byte_range().end;
                            }
                            for annotation in &ruby.annotations {
                                let (text, bidi) =
                                    content_to_runs(&annotation.content, fctx, new_style.clone())?;
                                run.annotations.push(RubyAnnotationSegment {
                                    level: annotation.level,
                                    bases: annotation.bases.clone(),
                                    text,
                                    bidi,
                                });
                            }
                            result.0.push(Run::Ruby(run));
                        }
                    }
                }
                Some(InlineItem::Text(text)) => {
                    let run = inline_runs_get_or_insert_text_nll_case_3!(
                        result,
                        text.content_range.clone()
                    );

                    run.range.end = text.content_range.end;
                    let mut current_paragraph = match bidi
                        .paragraphs
                        .binary_search_by_key(&text.content_range.start, |p| p.range.start)
                    {
                        Ok(i) => i,
                        Err(i) => i - 1,
                    };

                    let mut push = |level: unicode_bidi::Level,
                                    range: Range<usize>|
                     -> Result<(), InlineLayoutError> {
                        let direction = if level.is_ltr() {
                            Direction::Ltr
                        } else {
                            Direction::Rtl
                        };

                        let font_matcher = text::FontMatcher::match_all(
                            style.get::<style::FontFamily>(),
                            text::FontStyle {
                                weight: style
                                    .get_copy_or::<style::FontWeight, _>(I16Dot16::new(400)),
                                italic: match style.get_copy_or_default::<style::FontStyle, _>() {
                                    FontSlant::Regular => false,
                                    FontSlant::Italic => true,
                                },
                            },
                            style.get_copy_or::<style::FontSize, _>(
                                I26Dot6::new(16) * fctx.layout.dpi as i32 / 72,
                            ),
                            fctx.layout.dpi,
                            fctx.font_arena,
                            fctx.layout.fonts,
                        )?;

                        fctx.shaping_buffer.reset();
                        fctx.shaping_buffer.guess_properties();
                        fctx.shaping_buffer.set_direction(direction);
                        // TODO: Maybe this should not include *all* context?
                        //       For example characters inserted in place of an inline atomic probably shouldn't be here.
                        //       I think this requires an extra pass for us to figure out the appropriate context window.
                        fctx.shaping_buffer
                            .add(&content.main_text_content, range.clone());
                        let glyphs = GlyphString::from_glyphs(
                            content.main_text_content.clone(),
                            fctx.shaping_buffer.shape(
                                font_matcher.iterator(),
                                fctx.font_arena,
                                fctx.layout.fonts,
                            )?,
                        );

                        run.segments.push(TextRunSegment {
                            font_matcher,
                            direction,
                            range,
                            glyphs,
                        });

                        Ok(())
                    };

                    let mut current_level = bidi.levels[text.content_range.start];
                    let mut last = text.content_range.start;
                    for (i, &level) in bidi.levels[text.content_range.clone()].iter().enumerate() {
                        if bidi.paragraphs[current_paragraph].range.end == i {
                            push(current_level, last..i)?;
                            last = i;
                            current_paragraph += 1;
                        } else if current_level != level {
                            push(current_level, last..i)?;
                            last = i;
                        }
                        current_level = level;
                    }

                    push(current_level, last..text.content_range.end)?;
                }
                None => break,
            }
        }

        Ok(())
    }

    visit(
        &mut result,
        &bidi,
        content,
        fctx,
        base_style,
        &mut 0,
        usize::MAX,
    )?;

    Ok((result, bidi))
}

#[derive(Debug)]
//. tmp pub
pub struct InlineRuns<'a, 'f>(Vec<Run<'a, 'f>>);

impl<'a, 'f> InlineRuns<'a, 'f> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Reorder runs according to `bidi`. `bidi` must be a [`BidiInfo`] of the original
    /// original text string from which these runs originated. `self` must currently be in
    /// logical order (not already reordered).
    ///
    /// [`BidiInfo`]: unicode_bidi::BidiInfo
    pub fn reorder(
        &mut self,
        sbr: &Subrandr,
        bidi: &unicode_bidi::BidiInfo,
        temporary: &mut InlineRuns<'a, 'f>,
    ) {
        let line_range = {
            if let (Some(first), Some(last)) = (self.0.first(), self.0.last()) {
                first.byte_range().start..last.byte_range().end
            } else {
                // There's nothing to reorder, at most we'll run into indexing errors if
                // somehow there are paragraphs but not runs so we must bail here.
                return;
            }
        };
        log_once_state!(bad_ruby_reordering);

        // The whole line only consists of LTR levels, hence no bidirectional reodering is
        // needed and we can skip all of this mess.
        if bidi.levels[line_range.clone()]
            .iter()
            .all(|level| level.is_ltr())
        {
            return;
        }

        let mut visual_runs = Vec::new();
        for paragraph in &bidi.paragraphs {
            if line_range.start <= paragraph.range.start || line_range.end >= paragraph.range.end {
                let (_, mut paragraph_runs) = bidi.visual_runs(
                    paragraph,
                    line_range.start.max(paragraph.range.start)
                        ..line_range.end.min(paragraph.range.end),
                );
                visual_runs.append(&mut paragraph_runs);
            }
        }

        for range in visual_runs {
            let start = match self
                .0
                .binary_search_by_key(&range.start, |r| r.byte_range().start)
            {
                Ok(i) => i,
                Err(i) => i - 1,
            };

            let mut i = start;
            while i < self.0.len() {
                let run = &mut self.0[i];
                let run_range = run.byte_range();
                if run_range.start > range.end {
                    break;
                }
                if range.start <= run_range.start && range.end >= run_range.end {
                    temporary.0.push(self.0.remove(i));
                    continue;
                }

                match run {
                    Run::Text(text) => {
                        let s_start = match text
                            .segments
                            .binary_search_by_key(&range.start, |s| s.range.start)
                        {
                            Ok(i) => i,
                            Err(_) => unreachable!(
                                "bidi reordering attempted to partially reorder a text segment"
                            ),
                        };
                        let s_end = match text
                            .segments
                            .binary_search_by_key(&range.end, |s| s.range.end)
                        {
                            Ok(i) => i + 1,
                            // Range ends after this text run, don't fail here
                            Err(i) if i == text.segments.len() => i,
                            Err(_) => unreachable!(
                                "bidi reordering attempted to partially reorder a text segment"
                            ),
                        };
                        let run = inline_runs_get_or_insert_text_nll_case_3!(
                            temporary,
                            text.segments[s_start].range.clone()
                        );
                        run.segments.extend(text.segments.drain(s_start..s_end));
                        i += 1;
                    }
                    Run::Ruby(_) => {
                        // TODO: This could be somewhat allowed, and may be actually
                        //       necessary for RTL ruby containers.
                        //       Maybe this could be chaTextRun<'f>nged even more and we could treat
                        //       ruby containers as opaque replaced elements with their own nested
                        //       InlineRuns... not sure but it would fix these bidi issues.
                        error!(
                            sbr,
                            once(bad_ruby_reordering),
                            concat!(
                                "Bidirectional reordering partially reordered a ruby container! ",
                                "This is not allowed, unexpected results may follow!"
                            )
                        );
                        temporary.0.push(self.0.remove(i));
                    }
                }
            }
        }

        std::mem::swap(&mut self.0, &mut temporary.0);
        temporary.0.clear();
    }
}

#[derive(Debug)]
enum Run<'a, 'f> {
    Text(TextRun<'f>),
    Ruby(RubyRun<'a, 'f>),
}

impl<'a, 'f> Run<'a, 'f> {
    fn byte_range(&self) -> Range<usize> {
        match self {
            Run::Text(text) => text.byte_range(),
            Run::Ruby(ruby) => ruby.byte_range(),
        }
    }
}

trait InlineRun: Sized {
    fn byte_range(&self) -> Range<usize>;

    fn break_off(&mut self, width: FixedL) -> Option<Self> {
        _ = width;
        None
    }
}

/// A run of uninterrupted text segments. Note that two [`TextRun`]s should never end up
/// adjacent in a single [`InlineRuns`] list to achieve correct line-breaking. This is
/// because runs have implicit line breaking opportunities in-between and such adjacent runs
/// would cause such an opportunity to be present where it otherwise wouldn't be.
#[derive(Debug)]
struct TextRun<'f> {
    range: Range<usize>,
    segments: Vec<TextRunSegment<'f>>,
}

impl<'f> InlineRun for TextRun<'f> {
    fn byte_range(&self) -> Range<usize> {
        self.range.clone()
    }
}

/// A single text segment part of a [`TextRun`], contains a single span of same-font same-direction text.
#[derive(Debug)]
struct TextRunSegment<'f> {
    font_matcher: FontMatcher<'f>,
    direction: Direction,
    /// The range of the original string this segment represents, used for bidirectional reordering.
    range: Range<usize>,
    glyphs: GlyphString<'f, Rc<str>>,
}

/// A ruby run created from a single ruby container, contains some amount of bases
/// and annotations that span any range of bases.
/// This run is line-broken by splitting it base-wise whenever ALL annotation levels
/// allow it, as the spec mandates. This means ruby bases and annotations are never
/// split internally and spanning annotations prevent line breaks between their bases.
#[derive(Debug)]
struct RubyRun<'a, 'f> {
    range: Range<usize>,
    bases: Vec<InlineRuns<'a, 'f>>,
    annotations: Vec<RubyAnnotationSegment<'a, 'f>>,
}

impl<'a, 'f> InlineRun for RubyRun<'a, 'f> {
    fn byte_range(&self) -> Range<usize> {
        self.range.clone()
    }
}

/// The level of a ruby annotation, represented as an 8-bit signed integer.
/// Negative values are used to represent levels below the current line and
/// positive ones to represent levels over the current line.
///
/// Inter-character annotations are not currently supported.
// TODO: Inter-character annotations
//       (NOTE: Chromium does not yet support them so we don't really have to yet)
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct RubyLevel(NonZero<i8>);

impl RubyLevel {
    pub fn new(level: NonZero<i8>) -> Self {
        Self(level)
    }
}

/// A single annotation represented as a level, a range of bases and some nested inline content.
#[derive(Debug)]
struct RubyAnnotationSegment<'a, 'f> {
    level: RubyLevel,
    bases: Range<usize>,
    text: InlineRuns<'a, 'f>,
    bidi: unicode_bidi::BidiInfo<'a>,
}

struct ShaperSegment<'f> {
    content: Content<'f>,
    end: usize,
}

enum Content<'f> {
    Text(TextContent<'f>),
    None,
}

struct TextContent<'f> {
    font_matcher: FontMatcher<'f>,
    internal_breaks_allowed: bool,
    ruby_annotation: Option<Box<RubyAnnotation<'f>>>,
}

struct RubyAnnotation<'f> {
    font_matcher: FontMatcher<'f>,
    input_index: usize,
    // Note: Text does not shape or form ligatures across ruby annotations or bases, even merged ones, due to bidi isolation. See § 3.5 Bidi Reordering and CSS Text 3 § 7.3 Shaping Across Element Boundaries.
    // ^^ This means we can treat all ruby annotation as completely separate pieces of text.
    text: Rc<str>,
}

pub struct MultilineTextShaper<'f> {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segments: Vec<ShaperSegment<'f>>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ShapedSegment<'f> {
    pub glyphs: GlyphString<'f, Rc<str>>,
    pub baseline_offset: Point2<I26Dot6>,
    pub logical_rect: Rect2<I26Dot6>,
    pub corresponding_input_segment: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedLine<'f> {
    pub segments: Vec<ShapedSegment<'f>>,
    pub bounding_rect: Rect2<I26Dot6>,
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error(transparent)]
    Metrics(#[from] text::FreeTypeError),
}

fn shape_simple_segment<'f>(
    text: Rc<str>,
    range: impl text::ItemRange,
    font_iterator: text::FontMatchIterator<'_, 'f>,
    font_arena: &'f FontArena,
    fonts: &mut FontDb,
) -> Result<(Vec<text::Glyph<'f>>, TextMetrics), LayoutError> {
    let primary = font_iterator
        .matcher()
        .primary(font_arena, fonts)
        .map_err(text::ShapingError::FontSelect)?;

    let glyphs = {
        let mut buffer = text::ShapingBuffer::new();
        buffer.reset();
        buffer.add(&text, range);
        let direction = buffer.guess_properties();
        if !direction.is_horizontal() {
            buffer.set_direction(direction.to_horizontal());
        }
        buffer.shape(font_iterator, font_arena, fonts)?
    };

    let mut metrics = text::compute_extents_ex(true, &glyphs)?;
    metrics.extend_by_font(primary);

    Ok((glyphs, metrics))
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWrapMode {
    /// Greedy line breaking.
    #[default]
    Normal,
}

#[derive(Debug, Clone, Copy)]
pub struct TextWrapOptions {
    pub mode: TextWrapMode,
    pub strictness: LineBreakStrictness,
    pub word_break: LineBreakWordOption,
}

impl Default for TextWrapOptions {
    fn default() -> Self {
        Self {
            mode: TextWrapMode::Normal,
            strictness: LineBreakStrictness::Normal,
            word_break: LineBreakWordOption::Normal,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineHeight {
    #[default]
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RubyBaseId(usize);

// Notes on ruby support:
// Follows https://www.w3.org/TR/css-ruby-1/
// Only a single annotation level is (currently) supported.
// Whitespace should be handled according to the spec before it is passed here.
// (I do not believe this is currently done correctly as of now though)
// Annotations are passed into the shaper already paired with appriopriate bases.
// Ruby bases and annotations forbid internal line wrapping.
// All ruby annotations have exactly one base.
// TODO: default ruby-align is "space-around", this means justification with extra
//       justification opportunities at the start and end of the text
//       justification is not yet implemented, implement it. (
//         with the generic "justification system" to make it simpler to do this,
//         it should probably work like MultilineTextShaper except it accepts glyphstrings?
//       )
// Chromium seems to lay out ruby text at the top of the current entire line box,
// *when the whole thing is in one block* but youtube uses inline-block so the sane
// layout is correct.

impl<'f> MultilineTextShaper<'f> {
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segments: Vec::new(),
            intra_font_segment_splits: Vec::new(),
        }
    }

    pub fn add_text(&mut self, mut text: &str, font_matcher: FontMatcher<'f>) {
        while let Some(nl) = text.find('\n') {
            self.text.push_str(&text[..nl]);
            self.explicit_line_bounaries.push(self.text.len());
            text = &text[nl + 1..];
        }
        self.text.push_str(text);

        if let Some(&mut ShaperSegment {
            content:
                Content::Text(TextContent {
                    font_matcher: ref last_matcher,
                    internal_breaks_allowed: true,
                    ruby_annotation: None,
                }),
            end: ref mut last_end,
        }) = self.segments.last_mut()
        {
            if last_matcher == &font_matcher {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font_matcher,
                internal_breaks_allowed: true,
                ruby_annotation: None,
            }),
            end: self.text.len(),
        });
    }

    // TODO: Maybe a better system should be devised than this.
    //       Potentially just track an arbitrary `usize` provided as input for each segment,
    //       would require some restructuring.
    pub fn skip_segment_for_output(&mut self) {
        self.segments.push(ShaperSegment {
            content: Content::None,
            end: self.text.len(),
        });
    }

    pub fn add_ruby_base(&mut self, text: &str, font_matcher: FontMatcher<'f>) -> RubyBaseId {
        let id = self.segments.len();

        self.text.push_str(text);
        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font_matcher,
                internal_breaks_allowed: false,
                ruby_annotation: None,
            }),
            end: self.text.len(),
        });

        RubyBaseId(id)
    }

    pub fn add_ruby_annotation(
        &mut self,
        base: RubyBaseId,
        text: impl Into<Rc<str>> + std::fmt::Debug,
        font_matcher: FontMatcher<'f>,
    ) {
        let index = self.segments.len() + self.intra_font_segment_splits.len();

        let ShaperSegment {
            content:
                Content::Text(TextContent {
                    internal_breaks_allowed: false,
                    ruby_annotation: ref mut ruby_annotation @ None,
                    ..
                }),
            ..
        } = self.segments[base.0]
        else {
            panic!("ruby annotation placed on non-ruby base segment or one that already has an annotation in multiline shaper");
        };

        *ruby_annotation = Some(Box::new(RubyAnnotation {
            font_matcher,
            input_index: index,
            text: text.into(),
        }));
        self.skip_segment_for_output();
    }

    pub fn shape(
        &mut self,
        line_alignment: HorizontalAlignment,
        wrap: TextWrapOptions,
        wrap_width: I26Dot6,
        line_height: LineHeight,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<(Vec<ShapedLine<'f>>, Rect2<I26Dot6>), LayoutError> {
        while self
            .explicit_line_bounaries
            .pop_if(|i| *i == self.text.len())
            .is_some()
        {}

        if self.text.is_empty() {
            return Ok((Vec::new(), Rect2::ZERO));
        }

        if self.segments.is_empty() {
            return Ok((Vec::new(), Rect2::ZERO));
        }

        let segmenter = icu_segmenter::LineSegmenter::new_auto_with_options({
            let mut options = LineBreakOptions::default();
            options.strictness = wrap.strictness;
            options.word_option = wrap.word_break;
            options
        });

        let text: Rc<str> = std::mem::take(&mut self.text).into();
        let mut lines: Vec<ShapedLine> = vec![];
        let mut current_line_y = I26Dot6::ZERO;
        let mut total_rect = Rect2::NOTHING;

        let mut current_explicit_line = 0;
        let mut current_segment = 0;
        let mut current_intra_split = 0;
        let mut last = 0;
        while current_explicit_line <= self.explicit_line_bounaries.len() {
            let mut line_boundary = self
                .explicit_line_bounaries
                .get(current_explicit_line)
                .copied()
                .unwrap_or(text.len());
            let mut annotation_segments: Vec<ShapedSegment> = Vec::new();
            let mut segments: Vec<ShapedSegment> = Vec::new();
            let mut current_x = I26Dot6::ZERO;

            let mut line_max_ascender = I26Dot6::ZERO;
            let mut line_min_descender = I26Dot6::ZERO;
            // TODO: Line height should actually be calculated with respect to the
            //       whole *inline box*!!! Not its fragments like we currently do.
            //       See <https://www.w3.org/TR/css-inline-3/#inline-height> which refers
            //       purely to "inline box"es and not their constituent fragments.
            let mut annotations_max_ascender = I26Dot6::ZERO;

            while self.segments[current_segment].end <= last {
                current_segment += 1;
            }

            let mut post_wrap_glyphs: Option<GlyphString<'f, Rc<str>>> = None;

            loop {
                let ShaperSegment {
                    content: ref segment,
                    end: font_boundary,
                } = self.segments[current_segment];

                let mut end = font_boundary.min(line_boundary);
                let segment_slice = last..end;

                match segment {
                    Content::None => {}
                    &Content::Text(TextContent {
                        ref font_matcher,
                        internal_breaks_allowed,
                        ref ruby_annotation,
                    }) => {
                        let primary = font_matcher
                            .primary(font_arena, fonts)
                            .map_err(text::ShapingError::FontSelect)?;

                        let (mut glyphs, mut extents) = match post_wrap_glyphs.take() {
                            Some(glyphs) => {
                                let mut metrics =
                                    text::compute_extents_ex(true, glyphs.iter_glyphs())?;
                                metrics.extend_by_font(primary);

                                (glyphs, metrics)
                            }
                            None => {
                                let (vec, metrics) = shape_simple_segment(
                                    text.clone(),
                                    segment_slice.clone(),
                                    font_matcher.iterator(),
                                    font_arena,
                                    fonts,
                                )?;
                                (GlyphString::from_glyphs(text.clone(), vec), metrics)
                            }
                        };

                        // TODO: Inter-inline-block line breaking.
                        if wrap.mode == TextWrapMode::Normal
                            && internal_breaks_allowed
                            && current_x + extents.paint_size.x > wrap_width
                        {
                            const MAX_TRIES: usize = 3;

                            let max_width = wrap_width - current_x;
                            // A MAX_TRIES-wide buffer for breaking opportunities.
                            let mut candidate_breaks = [last; MAX_TRIES];
                            let breaks = segmenter.segment_str(&text[segment_slice.clone()]);
                            let mut glyph_it = glyphs.iter_glyphs().peekable();

                            let mut pos = I26Dot6::ZERO;
                            for offset in breaks {
                                let cluster = offset + segment_slice.start;

                                while let Some(glyph) =
                                    glyph_it.next_if(|glyph| glyph.cluster < cluster)
                                {
                                    pos += glyph.x_advance;
                                }

                                if pos > max_width {
                                    break;
                                } else {
                                    for i in (1..MAX_TRIES).rev() {
                                        candidate_breaks[i] = candidate_breaks[i - 1];
                                    }
                                    candidate_breaks[0] = cluster;
                                }
                            }

                            for candidate in candidate_breaks {
                                if candidate == last {
                                    continue;
                                }

                                if let Some((broken, remaining)) = glyphs.break_at_if_less_or_eq(
                                    candidate,
                                    max_width,
                                    &mut text::ShapingBuffer::new(),
                                    font_matcher.iterator(),
                                    font_arena,
                                    fonts,
                                )? {
                                    drop(glyph_it);
                                    glyphs = broken;
                                    post_wrap_glyphs = Some(remaining);
                                    end = candidate;
                                    line_boundary = candidate;

                                    extents = text::compute_extents_ex(true, glyphs.iter_glyphs())?;
                                    extents.extend_by_font(primary);

                                    break;
                                }
                            }
                        }

                        match line_height {
                            LineHeight::Normal => {
                                let line_gap = primary.metrics().line_gap();

                                extents.max_ascender += line_gap / 2;
                                extents.min_descender -= line_gap / 2;
                            }
                        }

                        line_max_ascender = line_max_ascender.max(extents.max_ascender);
                        line_min_descender = line_min_descender.min(extents.min_descender);

                        let logical_height = extents.max_ascender - extents.min_descender;

                        let ruby_padding = if let Some(annotation) = ruby_annotation {
                            let (glyphs, ruby_metrics) = shape_simple_segment(
                                annotation.text.clone(),
                                ..,
                                annotation.font_matcher.iterator(),
                                font_arena,
                                fonts,
                            )?;

                            let base_width = extents.paint_size.x + extents.trailing_advance;
                            let ruby_width =
                                ruby_metrics.paint_size.x + ruby_metrics.trailing_advance;
                            let (base_padding, ruby_padding) = if ruby_width > base_width {
                                ((ruby_width - base_width) / 2, I26Dot6::ZERO)
                            } else {
                                (I26Dot6::ZERO, (base_width - ruby_width) / 2)
                            };

                            annotations_max_ascender =
                                annotations_max_ascender.max(ruby_metrics.max_ascender);

                            // FIXME: Annotations seem to be slightly above where they should and
                            //        the logical rects also appear to be slightly too high.
                            annotation_segments.push(ShapedSegment {
                                glyphs: GlyphString::from_glyphs(text.clone(), glyphs),
                                baseline_offset: Point2::new(
                                    current_x + ruby_padding,
                                    current_line_y
                                        - extents.max_ascender
                                        - ruby_metrics.min_descender,
                                ),
                                logical_rect: Rect2::new(
                                    Point2::new(-ruby_padding, -ruby_metrics.max_ascender),
                                    Point2::new(
                                        ruby_metrics.paint_size.x
                                            + ruby_metrics.trailing_advance
                                            + ruby_padding,
                                        -ruby_metrics.min_descender,
                                    ),
                                ),
                                corresponding_input_segment: annotation.input_index,
                            });

                            base_padding
                        } else {
                            I26Dot6::ZERO
                        };

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            let logical_width =
                                extents.paint_size.x + extents.trailing_advance + ruby_padding * 2;
                            segments.push(ShapedSegment {
                                glyphs,
                                baseline_offset: Point2::new(
                                    current_x + ruby_padding,
                                    current_line_y,
                                ),
                                logical_rect: Rect2::from_min_size(
                                    Point2::new(-ruby_padding, -extents.max_ascender),
                                    Vec2::new(logical_width, logical_height),
                                ),
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                            current_x += logical_width;
                        } else {
                            assert_eq!(
                                ruby_padding,
                                I26Dot6::ZERO,
                                "ruby bases cannot have internal segment splits"
                            );

                            loop {
                                let split_end = self
                                    .intra_font_segment_splits
                                    .get(current_intra_split)
                                    .copied()
                                    .unwrap_or(end);
                                let glyph_slice = match glyphs.split_off_until_cluster(split_end) {
                                    Some(string) => string,
                                    None => break,
                                };
                                let local_max_ascender = extents.max_ascender;
                                let extents =
                                    text::compute_extents_ex(true, glyph_slice.iter_glyphs())?;

                                segments.push(ShapedSegment {
                                    glyphs: glyph_slice,
                                    baseline_offset: Point2::new(current_x, current_line_y),
                                    logical_rect: Rect2::from_min_size(
                                        Point2::new(I26Dot6::ZERO, -local_max_ascender),
                                        Vec2::new(
                                            extents.paint_size.x + extents.trailing_advance,
                                            logical_height,
                                        ),
                                    ),
                                    corresponding_input_segment: current_segment
                                        + current_intra_split,
                                });
                                current_x += extents.paint_size.x + extents.trailing_advance;

                                if split_end >= end {
                                    break;
                                } else {
                                    current_intra_split += 1;
                                }
                            }
                        }
                    }
                }

                last = end;

                if end == line_boundary {
                    if post_wrap_glyphs.is_none() {
                        current_explicit_line += 1;
                    }
                    break;
                } else {
                    current_segment += 1;
                }
            }

            debug_assert_eq!(last, line_boundary);

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => I26Dot6::ZERO,
                HorizontalAlignment::Center => -current_x / 2,
                HorizontalAlignment::Right => -current_x,
            };

            let annotation_y_adjustment = if current_line_y == I26Dot6::ZERO {
                I26Dot6::ZERO
            } else {
                annotations_max_ascender
            };

            let final_line_height = line_max_ascender - line_min_descender;

            current_line_y += annotation_y_adjustment;

            for segment in segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                segment.baseline_offset.y += line_max_ascender + annotation_y_adjustment;
                segment.logical_rect = segment.logical_rect.translate(Vec2::new(
                    segment.baseline_offset.x,
                    current_line_y + line_max_ascender,
                ));
            }

            for segment in annotation_segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                segment.baseline_offset.y += line_max_ascender + annotation_y_adjustment;
                segment.logical_rect = segment
                    .logical_rect
                    .translate(segment.baseline_offset.to_vec());
            }

            let mut line_rect = Rect2::NOTHING;
            for segment in &segments {
                total_rect.expand_to_rect(segment.logical_rect);
                line_rect.expand_to_rect(segment.logical_rect);
            }

            current_line_y += final_line_height;

            segments.append(&mut annotation_segments);

            lines.push(ShapedLine {
                segments,
                bounding_rect: line_rect,
            });
        }

        Ok((lines, total_rect))
    }
}

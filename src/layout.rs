use std::cmp::Reverse;

use crate::{
    math::{I16Dot16, I26Dot6, Vec2},
    text::{self, FontSelect},
};

// https://www.w3.org/TR/css-inline-3/#model

struct Element {}

enum Box {
    Text(Text),
    AtomicInline(AtomicInline),
}

enum Baseline {
    Alphabetic,
    CapHeight,
    XHeight,
    XMiddle,
    IdeographicOver,
    IdeographicUnder,
    Central,
    IdeographInkOver,
    IdeographicInkUnder,
    Hanging,
    Math,
}

impl Baseline {
    fn ascender_for_font(self, font: &text::Font) -> I26Dot6 {
        let metrics = font.metrics();
        match self {
            Baseline::Alphabetic => metrics.ascender,
            Baseline::CapHeight => todo!(),
            Baseline::XHeight => todo!(),
            Baseline::XMiddle => {
                (Baseline::Alphabetic.ascender_for_font(font)
                    + Baseline::XHeight.ascender_for_font(font))
                    / 2
            }
            Baseline::IdeographicOver => todo!(),
            Baseline::IdeographicUnder => todo!(),
            Baseline::Central => {
                (Baseline::IdeographicOver.ascender_for_font(font)
                    + Baseline::IdeographicUnder.ascender_for_font(font))
                    / 2
            }
            Baseline::IdeographInkOver => todo!(),
            Baseline::IdeographicInkUnder => todo!(),
            Baseline::Hanging => todo!(),
            Baseline::Math => todo!(),
        }
    }
}

enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

// size: Vec2<I26Dot6>,
// ascender: I26Dot6,

fn is_generic_family_keyword(family: &str) -> bool {
    match family {
        "serif" | "sans-serif" | "system-ui" | "cursive" | "fantasy" | "math" | "monospace"
        | "ui-serif" | "ui-sans-serif" | "ui-monospace" | "ui-rounded" => true,
        // TODO: I am pretty certain fontconfig will not return anything useful for these
        //       should they be implemented separately somehow or something?
        "generic(fangsong)" | "generic(kai)" | "generic(khmer-mul)" | "generic(nastaliq)" => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
// TODO: Width and obliqueness not supported yet
// FIXME: public only for testing
pub struct FontStyle {
    pub weight: I16Dot16,
    pub italic: bool,
}

// This function actually implements the logic from level 4
// https://drafts.csswg.org/css-fonts/#font-style-matching
fn match_faces_for_weight(
    faces: &mut [text::FaceInfo],
    style: &FontStyle,
    fonts: &mut FontSelect,
) -> Result<Option<text::Face>, text::font_select::Error> {
    if faces.is_empty() {
        return Ok(None);
    }

    macro_rules! sort_faces {
        (ascending) => {
            faces.sort_unstable_by_key(|face| match face.weight {
                text::FontAxisValues::Fixed(fixed) => fixed,
                text::FontAxisValues::Range(start, _) => start,
            });
        };
        (descending) => {
            faces.sort_unstable_by_key(|face| {
                Reverse(match face.weight {
                    text::FontAxisValues::Fixed(fixed) => fixed,
                    text::FontAxisValues::Range(_, end) => end,
                })
            });
        };
    }

    macro_rules! open_face {
        ($face: expr, $weight: expr) => {
            let mut result = fonts.open(&$face)?;
            text::font_select::set_weight_if_variable(&mut result, $weight);
            return Ok(Some(result));
        };
    }

    macro_rules! check_weights_in_range {
        ($order: ident, ($start: expr) ..= ($end: expr)) => {{
            let mut min_1 = None;
            for face in faces.iter() {
                let wght = check_weights_in_range!(@get_weight $order face $start, $end);
                if face.weight.contains(wght) {
                    min_1 = Some((face, wght))
                }
            }

            if let Some((face, weight)) = min_1 {
                open_face!(face, weight);
            }
        }};
        (@get_weight ascending $face: ident $start: expr, $end: expr) => {
            $face.weight.minimum().clamp($start, $end)
        };
        (@get_weight descending $face: ident $start: expr, $end: expr) => {
            $face.weight.maximum().clamp($start, $end)
        };
    }

    // Given the desired weight and the weights of faces in the matching set after the steps above,
    // if the desired weight is available that face matches.
    if let Some(face) = faces.iter().find(|face| face.weight.contains(style.weight)) {
        open_face!(face, style.weight);
    }

    if style.weight > 400 && style.weight < 500 {
        // If the desired weight is inclusively between 400 and 500, weights greater than or equal to the target weight are checked in ascending order until 500 is hit and checked,
        sort_faces!(ascending);
        check_weights_in_range!(ascending, (style.weight)..=(I16Dot16::new(500)));

        // followed by weights less than the target weight in descending order,
        sort_faces!(descending);
        check_weights_in_range!(descending, (I16Dot16::new(1))..=(style.weight));

        // followed by weights greater than 500, until a match is found.
        sort_faces!(ascending);
        check_weights_in_range!(ascending, (I16Dot16::new(500))..=(I16Dot16::new(1000)));
    } else if style.weight < 400 {
        // If the desired weight is less than 400, weights less than or equal to the desired weight are checked in descending order
        sort_faces!(descending);
        check_weights_in_range!(descending, (I16Dot16::new(1))..=(style.weight));

        // followed by weights above the desired weight in ascending order until a match is found.
        sort_faces!(ascending);
        check_weights_in_range!(ascending, (style.weight)..=(I16Dot16::new(1000)));
    } else {
        // If the desired weight is greater than 500, weights greater than or equal to the desired weight are checked in ascending order
        sort_faces!(ascending);
        check_weights_in_range!(ascending, (style.weight)..=(I16Dot16::new(1000)));

        // followed by weights below the desired weight in descending order until a match is found.
        sort_faces!(descending);
        check_weights_in_range!(descending, (I16Dot16::new(1))..=(style.weight));
    }

    // This may still happen if all faces have a weight value outside 1..=1000 for whatever reason.
    Ok(None)
}

fn match_faces(
    faces: &[text::FaceInfo],
    style: &FontStyle,
    fonts: &mut FontSelect,
) -> Result<Option<text::Face>, text::font_select::Error> {
    let order = if style.italic {
        [true, false]
    } else {
        [false, true]
    };

    for want_italic in order {
        let mut faces = faces
            .iter()
            .filter(|face| face.italic == want_italic)
            .cloned()
            .collect::<Vec<_>>();

        if let Some(face) = match_faces_for_weight(&mut faces, style, fonts)? {
            return Ok(Some(face));
        }
    }

    Ok(None)
}

fn match_face_for_specific_family(
    chr: Option<char>,
    family: &str,
    style: &FontStyle,
    fonts: &mut FontSelect,
) -> Result<Option<text::Face>, text::font_select::Error> {
    if is_generic_family_keyword(family) {
        // FIXME: This is incorrect, we should just subtitute the *family name*, nothing else.
        return fonts.try_select(&text::FontRequest {
            families: vec![(*family).into()],
            weight: style.weight,
            italic: style.italic,
            codepoint: chr.map(Into::into),
        });
    }

    // FIXME: Fontconfig for 99% does not use the specified case folded comparison.
    //        This is done this way to allow for font family substitutions to still happen.
    //        We should probably just handle substitutions ourselves.
    let faces = fonts.query_by_name(family)?.to_vec();

    // If no matching face exists or the matched face does not contain a glyph for the character to be rendered, the next family name is selected and the previous three steps repeated. Glyphs from other faces in the family are not considered. The only exception is that user agents may optionally substitute a synthetically obliqued version of the default face if that face supports a given glyph and synthesis of these faces is permitted by the value of the ‘font-synthesis’ property. For example, a synthetic italic version of the regular face may be used if the italic face doesn't support glyphs for Arabic.
    let Some(face) = match_faces(&faces, style, fonts)? else {
        return Ok(None);
    };

    if let Some(chr) = chr {
        if !face.contains_character(chr) {
            return Ok(None);
        }
    }

    Ok(Some(face))
}
// TODO: this doesn't consider style of non-scalable fonts
//       (point 4.d in the spec)
fn match_face_for(
    chr: Option<char>,
    families: &[&str],
    style: &FontStyle,
    fonts: &mut FontSelect,
) -> Result<Option<text::Face>, text::font_select::Error> {
    let codepoint = chr.map(char::into);

    for &family in families {
        if let Some(face) = match_face_for_specific_family(chr, family, style, fonts)? {
            return Ok(Some(face));
        }
    }

    // If there are no more font families to be evaluated and no matching face has been found, then the user agent performs a system font fallback procedure to find the best match for the character to be rendered. The result of this procedure may vary across user agents.
    fonts.try_select(&text::FontRequest {
        families: families.iter().map(|s| (*s).into()).collect(),
        weight: style.weight,
        italic: style.italic,
        codepoint,
    })
}

fn match_font_for_single_character(
    chr: char,
    families: &[&str],
    style: &FontStyle,
    size: I16Dot16,
    fonts: &mut FontSelect,
) -> Result<text::Font, text::font_select::Error> {
    // FIXME: Spec says to use a fallback font. Decide what to do in this case.
    let face = match_face_for(Some(chr), families, style, fonts)?
        .ok_or(text::font_select::Error::NotFound)?;

    // TODO: pass dpi all the way down to here...
    let font = face.with_size(size.into_f32(), 72);

    Ok(font)
}

fn match_font_for_cluster(
    list: &mut FontList,
    offset: usize,
    cluster: &str,
    families: &[&str],
    style: &FontStyle,
    size: I16Dot16,
    fonts: &mut FontSelect,
) -> Result<(), text::font_select::Error> {
    let mut chars = cluster.chars();
    let base = chars.next().unwrap();
    if chars.as_str().is_empty() {
        let font = match_font_for_single_character(base, families, style, size, fonts)?;

        list.push(offset + cluster.len(), font);
        Ok(())
    } else {
        // For each family in the font list, a face is chosen using the style selection rules defined in the previous section.
        for family in families {
            let Some(face) = match_face_for_specific_family(Some(base), family, style, fonts)?
            else {
                continue;
            };

            // TODO: If a sequence of multiple codepoints is canonically equivalent to a single character and the font supports that character, select this font for the sequence and use the glyph associated with the canonically equivalent character for the entire cluster.

            for chr in chars.clone() {
                if !face.contains_character(chr) {
                    continue;
                }
            }

            // If all characters in the sequence b + c1 + c2 … are completely supported by the font, select this font for the sequence.
            list.push(offset + cluster.len(), face.with_size(size.into_f32(), 72));
            return Ok(());
        }

        // If no font was found in the font list in step 1:
        {
            // If c1 is a variation selector, system fallback must be used to find a font that supports the full sequence of b + c1. If no font on the system supports the full sequence, match the single character b using the normal procedure for matching single characters and ignore the variation selector. Note: a sequence with more than one variation selector must be treated as an encoding error and the trailing selectors must be ignored. [UNICODE]

            if icu_properties::sets::variation_selector().contains(chars.next().unwrap()) {
                // TODO: Matching `b + c1` like this is not supported by FontSelect right now
                //       Maybe FT_Face_GetCharVariantIndex should be used for this?
                let font = match_font_for_single_character(base, families, style, size, fonts)?;
                list.push(offset + cluster.len(), font);
                return Ok(());
            }

            // Otherwise, the user agent may optionally use system font fallback to match a font that supports the entire cluster.
            // TODO: *optionally* use system font fallback to match a font that supports the entire cluster.
        }

        // If no font is found in step 2, use the matching sequence from step 1 to determine the longest sequence that is completely supported by a font in the font list
        let mut max = None;
        for family in families {
            let Some(face) = match_face_for_specific_family(Some(base), family, style, fonts)?
            else {
                continue;
            };

            // TODO: Does this still apply here?
            //       If a sequence of multiple codepoints is canonically equivalent to a single character and the font supports that character, select this font for the sequence and use the glyph associated with the canonically equivalent character for the entire cluster.

            let length = chars
                .clone()
                .take_while(|&chr| face.contains_character(chr))
                .count();

            if max.as_ref().is_none_or(|&(prev, _)| prev < length) {
                max = Some((length, face))
            }
        }

        if let Some((len, face)) = max {
            list.push(offset + len, face.with_size(size.into_f32(), 72));

            // and attempt to match the remaining combining characters separately using the rules for single characters.
            let mut current = offset + len;
            for chr in chars {
                let font = match_font_for_single_character(chr, families, style, size, fonts)?;
                let chr_len = chr.len_utf8();
                current += chr_len;
                list.push(current, font);
            }

            return Ok(());
        }

        // FIXME: Spec doesn't actually say what to do here, but the answer is probably to use a fallback font.
        return Err(text::font_select::Error::NotFound);
    }
}

#[derive(Debug)]
pub struct FontList(Vec<(usize, text::Font)>);

impl FontList {
    // FIXME: public only for testing
    pub fn new() -> Self {
        Self(Vec::new())
    }

    fn push(&mut self, until: usize, font: text::Font) {
        if let Some(last) = self.0.last_mut().filter(|last| last.1 == font) {
            last.0 = until;
        } else {
            self.0.push((until, font));
        }
    }
}

pub fn match_fonts_for_text(
    list: &mut FontList,
    offset: usize,
    text: &str,
    families: &[&str],
    style: &FontStyle,
    size: I16Dot16,
    fonts: &mut FontSelect,
) -> Result<(), text::font_select::Error> {
    let segmenter = icu_segmenter::GraphemeClusterSegmenter::new();

    let mut last = 0;
    for end in segmenter.segment_str(text).skip(1) {
        let cluster = &text[last..end];

        match_font_for_cluster(list, last + offset, cluster, families, style, size, fonts)?;

        last = end;
    }

    Ok(())
}

struct Text {}

struct AtomicInline {
    size: Vec2<I26Dot6>,
}

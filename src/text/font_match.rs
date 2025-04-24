use std::cmp::Reverse;

use crate::{math::I16Dot16, I26Dot6};

use super::{font_db, Face, FaceInfo, Font, FontArena, FontDb, FontStyle};

// This function actually implements the logic from level 4
// https://drafts.csswg.org/css-fonts/#font-style-matching
fn match_faces_for_weight(
    faces: &mut [FaceInfo],
    style: &FontStyle,
    fonts: &mut FontDb,
) -> Result<Option<Face>, font_db::SelectError> {
    if faces.is_empty() {
        return Ok(None);
    }

    macro_rules! sort_faces {
        (ascending) => {
            faces.sort_unstable_by_key(|face| match face.weight {
                font_db::FontAxisValues::Fixed(fixed) => fixed,
                font_db::FontAxisValues::Range(start, _) => start,
            });
        };
        (descending) => {
            faces.sort_unstable_by_key(|face| {
                Reverse(match face.weight {
                    font_db::FontAxisValues::Fixed(fixed) => fixed,
                    font_db::FontAxisValues::Range(_, end) => end,
                })
            });
        };
    }

    macro_rules! open_face {
        ($face: expr, $weight: expr) => {
            let mut result = fonts.open(&$face)?;
            font_db::set_weight_if_variable(&mut result, $weight);
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
    faces: &[FaceInfo],
    style: &FontStyle,
    fonts: &mut FontDb,
) -> Result<Option<Face>, font_db::SelectError> {
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
    family: &str,
    style: &FontStyle,
    fonts: &mut FontDb,
) -> Result<Option<Face>, font_db::SelectError> {
    // FIXME: Fontconfig for 99% does not use the specified case folded comparison.
    //        This is done this way to allow for font family substitutions to still happen.
    //        We should probably just handle substitutions ourselves?
    //        Although I think browsers just ignore this maybe??? Not sure.
    let faces = fonts.query_by_name(family)?.to_vec();

    // If no matching face exists or the matched face does not contain a glyph for the character to be rendered, the next family name is selected and the previous three steps repeated. Glyphs from other faces in the family are not considered. The only exception is that user agents may optionally substitute a synthetically obliqued version of the default face if that face supports a given glyph and synthesis of these faces is permitted by the value of the ‘font-synthesis’ property. For example, a synthetic italic version of the regular face may be used if the italic face doesn't support glyphs for Arabic.
    let Some(face) = match_faces(&faces, style, fonts)? else {
        return Ok(None);
    };

    Ok(Some(face))
}

#[derive(Debug, PartialEq, Eq)]
pub struct FontMatcher<'f> {
    families: Vec<Box<str>>,
    style: FontStyle,
    size: I26Dot6,
    dpi: u32,
    matched: Vec<&'f Font>,
}

impl<'f> FontMatcher<'f> {
    pub fn match_all(
        families: impl IntoIterator<Item = impl AsRef<str>>,
        style: FontStyle,
        size: I26Dot6,
        dpi: u32,
        arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Self, font_db::SelectError> {
        let mut copied_families = Vec::new();
        let mut matched = Vec::new();

        for family in families {
            let family = family.as_ref();
            copied_families.push(family.into());
            if let Some(face) = match_face_for_specific_family(family, &style, fonts)? {
                matched.push(arena.insert(&face.with_size(size, dpi)?));
            }
        }

        Ok(Self {
            families: copied_families,
            style,
            size,
            dpi,
            matched,
        })
    }

    pub fn iterator(&self) -> FontMatchIterator<'_, 'f> {
        FontMatchIterator {
            matcher: self,
            index: 0,
        }
    }

    // TODO: Make something like a TofuFont that would be a virtual font that is always
    //       available, then no extra optional handling would have to be done.
    pub fn primary(
        &self,
        arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Option<&'f Font>, font_db::SelectError> {
        self.iterator().next_with_fallback(' '.into(), arena, fonts)
    }
}

#[derive(Debug, Clone)]
pub struct FontMatchIterator<'a, 'f> {
    matcher: &'a FontMatcher<'f>,
    index: usize,
}

impl<'a, 'f> FontMatchIterator<'a, 'f> {
    pub fn matcher(&self) -> &FontMatcher<'f> {
        self.matcher
    }

    pub fn did_system_fallback(&self) -> bool {
        self.index > self.matcher.matched.len()
    }

    pub fn next_with_fallback(
        &mut self,
        codepoint: u32,
        arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Option<&'f Font>, font_db::SelectError> {
        match self.matcher.matched.get(self.index) {
            Some(&result) => {
                self.index += 1;
                return Ok(Some(result));
            }
            None => {
                if self.index == self.matcher.matched.len() {
                    self.index += 1;
                }

                match fonts.select(&super::FontRequest {
                    families: self.matcher.families.clone(),
                    style: self.matcher.style,
                    codepoint: Some(codepoint),
                }) {
                    Ok(face) => Ok(Some(
                        arena.insert(&face.with_size(self.matcher.size, self.matcher.dpi)?),
                    )),
                    Err(super::SelectError::NotFound) => Ok(None),
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

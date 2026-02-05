use std::cmp::Reverse;

use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
};

use super::{font_db, Face, FaceInfo, Font, FontDb, FontFallbackRequest, FontStyle};

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

pub(super) fn match_faces(
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMatcher {
    families: Rc<[Rc<str>]>,
    style: FontStyle,
    size: I26Dot6,
    dpi: u32,
}

impl FontMatcher {
    pub fn new(families: Rc<[Rc<str>]>, style: FontStyle, size: I26Dot6, dpi: u32) -> Self {
        Self {
            families,
            style,
            size,
            dpi,
        }
    }

    pub fn iterator(&self) -> FontMatchIterator<'_> {
        FontMatchIterator {
            matcher: self,
            index: 0,
        }
    }

    pub fn tofu(&self) -> Font {
        Face::tofu().with_size(self.size, self.dpi).unwrap()
    }

    pub fn size(&self) -> I26Dot6 {
        self.size
    }

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    // TODO: Note: it does not matter whether that font actually has a glyph for the space character.
    //       ^^^^ The current implementation might not interact well with font fallback in this regard
    pub fn primary(&self, fonts: &mut FontDb) -> Result<Font, font_db::SelectError> {
        Ok(self
            .iterator()
            .next_with_fallback(' '.into(), fonts)?
            .unwrap_or_else(|| self.tofu()))
    }
}

#[derive(Debug, Clone)]
pub struct FontMatchIterator<'a> {
    matcher: &'a FontMatcher,
    index: usize,
}

impl FontMatchIterator<'_> {
    pub fn matcher(&self) -> &FontMatcher {
        self.matcher
    }

    pub fn did_system_fallback(&self) -> bool {
        self.index > self.matcher.families.len()
    }

    pub fn next_with_fallback(
        &mut self,
        codepoint: u32,
        fonts: &mut FontDb,
    ) -> Result<Option<Font>, font_db::SelectError> {
        loop {
            match self.matcher.families.get(self.index) {
                Some(family) => {
                    self.index += 1;

                    let Some(matched) =
                        fonts.match_face_for_family(family.clone(), self.matcher.style)?
                    else {
                        continue;
                    };

                    return Ok(Some(
                        matched.with_size(self.matcher.size, self.matcher.dpi)?,
                    ));
                }
                None => {
                    if self.index == self.matcher.families.len() {
                        self.index += 1;
                    }

                    return match fonts.select_fallback(&FontFallbackRequest {
                        families: self
                            .matcher
                            .families
                            .iter()
                            .map(|x| (&**x).into())
                            .collect(),
                        style: self.matcher.style,
                        codepoint,
                    }) {
                        Ok(face) => Ok(Some(face.with_size(self.matcher.size, self.matcher.dpi)?)),
                        Err(super::SelectError::NotFound) => Ok(None),
                        Err(err) => Err(err),
                    };
                }
            }
        }
    }
}

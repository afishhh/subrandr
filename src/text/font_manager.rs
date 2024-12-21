use std::{borrow::Borrow, collections::HashMap, hash::Hash};

use crate::{
    math::Fixed,
    util::{AnyError, Sealed},
};

use super::*;

// TODO: Is the `ital` axis worth supporting?
//       I don't even think I have one like that so I could use it for testing...
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct FamilySlot {
    variable_weight: Option<(Face, usize)>,
    // NOTE: This is a variable italic variant, not a font with an ital axis.
    italic_variable_weight: Option<(Face, usize)>,
    /// The currently loaded non-variadic faces of this family.
    variants: HashMap<(/* weight */ FT_Fixed, /* italic */ bool), Face>,
}

impl FamilySlot {
    fn add_font(&mut self, face: Face, weight: f32, italic: bool) {
        if let Some(weight_axis) = face.axis(WEIGHT_AXIS) {
            let face_and_axis = (face, weight_axis.index);
            if italic {
                self.italic_variable_weight.get_or_insert(face_and_axis);
            } else {
                self.variable_weight.get_or_insert(face_and_axis);
            }
        } else {
            self.variants
                .entry((f32_to_fixed_point(weight), italic))
                .or_insert(face);
        }
    }

    fn find(&self, weight: f32, italic: bool) -> Option<Face> {
        let variable_weight = if italic {
            &self.italic_variable_weight
        } else {
            &self.variable_weight
        };

        if let Some((face, axis_index)) = variable_weight {
            let mut face = face.clone();
            face.set_axis(*axis_index, weight);
            Some(face)
        } else {
            self.variants
                .get(&(f32_to_fixed_point(weight), italic))
                .cloned()
        }
    }
}

fn set_weight_if_variable(face: &mut Face, weight: f32) {
    if let Some(axis) = face.axis(WEIGHT_AXIS) {
        face.set_axis(axis.index, weight)
    }
}

pub trait FontBackend: Sealed + std::fmt::Debug {
    fn load_fallback(&mut self, weight: f32, italic: bool) -> Result<Option<Face>, AnyError>;
    fn load(&mut self, name: &str, weight: f32, italic: bool) -> Result<Option<Face>, AnyError>;
    fn load_glyph_fallback(
        &mut self,
        weight: f32,
        italic: bool,
        codepoint: u32,
    ) -> Result<Option<Face>, AnyError>;
}

#[derive(Debug)]
struct FamilyMap<K>(HashMap<K, FamilySlot>);

impl<K: Hash + Eq> FamilyMap<K> {
    fn get_mut<Q: ?Sized>(&mut self, name: &Q) -> &mut FamilySlot
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K>,
    {
        // SAFETY: In the else branch self is not actually borrowed anymore but
        //         borrowchk is unable to see that data dependent lifetime.
        unsafe {
            let self_: *mut Self = self as *mut Self;

            if let Some(slot) = (*self_).0.get_mut(name) {
                slot
            } else {
                (*self_)
                    .0
                    .entry(name.to_owned())
                    .insert_entry(FamilySlot::default())
                    .into_mut()
            }
        }
    }
}

#[derive(Debug)]
pub struct FontManager {
    families: FamilyMap<String>,
    codepoint_fallbacks: HashMap<(Fixed<16>, bool, u32), Option<Face>>,
    fallback: FamilySlot,
    backend: Box<dyn FontBackend>,
}

impl FontManager {
    pub fn new(backend: Box<dyn FontBackend>) -> Self {
        Self {
            families: FamilyMap(HashMap::new()),
            codepoint_fallbacks: HashMap::new(),
            fallback: FamilySlot::default(),
            backend,
        }
    }

    pub fn insert(&mut self, name: &str, face: Face, weight: f32, italic: bool) -> &mut FamilySlot {
        let slot = self.families.get_mut(name);
        slot.add_font(face, weight, italic);
        slot
    }

    fn get_internal(
        &mut self,
        name: &str,
        weight: f32,
        italic: bool,
        load: bool,
    ) -> Result<Face, AnyError> {
        let slot = self.families.get_mut(name);

        if let Some(face) = slot.find(weight, italic) {
            return Ok(face);
        }

        if load {
            match self.backend.load(name, weight, italic) {
                Ok(Some(mut f)) => {
                    slot.add_font(f.clone(), weight, italic);
                    set_weight_if_variable(&mut f, weight);
                    Ok(f)
                }
                Ok(None) => {
                    let mut fallback =
                        self.backend.load_fallback(weight, italic)?.ok_or_else(|| {
                            AnyError::from("Fallback font with specified coordinates not found")
                        })?;
                    self.fallback.add_font(fallback.clone(), weight, italic);
                    set_weight_if_variable(&mut fallback, weight);
                    Ok(fallback)
                }
                Err(e) => Err(e),
            }
        } else {
            Err("Font not found".into())
        }
    }

    pub fn get(&mut self, name: &str, weight: f32, italic: bool) -> Result<Face, AnyError> {
        self.get_internal(name, weight, italic, false)
    }

    pub fn get_or_load(&mut self, name: &str, weight: f32, italic: bool) -> Result<Face, AnyError> {
        self.get_internal(name, weight, italic, true)
    }

    pub fn get_or_load_fallback_for(
        &mut self,
        weight: f32,
        italic: bool,
        codepoint: u32,
    ) -> Result<Option<Face>, AnyError> {
        let key = (Fixed::from_f32(weight), italic, codepoint);
        if let Some(result) = self.codepoint_fallbacks.get(&key) {
            Ok(result.clone())
        } else {
            let result = match self
                .backend
                .load_glyph_fallback(weight, italic, codepoint)?
            {
                Some(mut f) => {
                    self.insert(f.family_name(), f.clone(), weight, italic);
                    set_weight_if_variable(&mut f, weight);
                    Some(f)
                }
                None => None,
            };

            self.codepoint_fallbacks.insert(key, result.clone());

            Ok(result)
        }
    }
}

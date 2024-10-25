use std::collections::HashMap;

use crate::util::{AnyError, Sealed};

use super::*;
use ft_utils::*;

// TODO: Is the `ital` axis worth supporting?
//       I don't even think I have one like that so I could use it for testing...
#[derive(Debug)]
#[doc(hidden)]
pub struct FamilySlot {
    variable_weight: Option<(Face, usize)>,
    // NOTE: This is a variable italic variant, not a font with an ital axis.
    italic_variable_weight: Option<(Face, usize)>,
    /// The currently loaded non-variadic faces of this family.
    variants: HashMap<(/* weight */ FT_Fixed, /* italic */ bool), Face>,
}

impl Default for FamilySlot {
    fn default() -> Self {
        Self {
            variable_weight: None,
            italic_variable_weight: None,
            variants: HashMap::new(),
        }
    }
}

impl FamilySlot {
    fn add_font(&mut self, face: Face, weight: f32, italic: bool) {
        if let Some(weight_axis) = face.axis(WEIGHT_AXIS) {
            let face_and_axis = (face.clone(), weight_axis.index);
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
            return Some(face);
        } else {
            return self
                .variants
                .get(&(f32_to_fixed_point(weight), italic))
                .cloned();
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
}

#[derive(Debug)]
struct FamilyMap(HashMap<String, FamilySlot>);

impl FamilyMap {
    fn get_mut(&mut self, name: &str) -> &mut FamilySlot {
        // SAFETY: In the else branch self is not actually borrowed anymore but
        //         borrowchk is unable to see that data dependent lifetime.
        unsafe {
            let self_: *mut Self = std::mem::transmute(self);

            if let Some(slot) = (*self_).0.get_mut(name) {
                slot
            } else {
                (*self_)
                    .0
                    .entry(name.to_string())
                    .insert_entry(FamilySlot::default())
                    .into_mut()
            }
        }
    }
}

#[derive(Debug)]
pub struct FontManager {
    families: FamilyMap,
    fallback: FamilySlot,
    backend: Box<dyn FontBackend>,
}

impl FontManager {
    pub fn new(backend: Box<dyn FontBackend>) -> Self {
        Self {
            families: FamilyMap(HashMap::new()),
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
                    return Ok(fallback);
                }
                Err(e) => Err(e),
            }
        } else {
            return Err("Font not found".into());
        }
    }

    pub fn get(&mut self, name: &str, weight: f32, italic: bool) -> Result<Face, AnyError> {
        self.get_internal(name, weight, italic, false)
    }

    pub fn get_or_load(&mut self, name: &str, weight: f32, italic: bool) -> Result<Face, AnyError> {
        self.get_internal(name, weight, italic, true)
    }
}

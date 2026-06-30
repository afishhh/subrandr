use std::{collections::HashMap, sync::LazyLock};

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use log::{warn, LogContext};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
    rc_static,
};

pub mod values;

pub mod computed;
use computed::*;

use crate::csssyn::value::Declaration;

trait ComputedProperty {
    type Value: Clone;
    const INHERITED: bool;
    fn get(style: &ComputedStyle) -> &Self::Value;
    fn set(style: &mut ComputedStyle, value: Self::Value);
}

// Generates `ComputedStyle`.
//
// `ComputedStyle` is bascially a tree of `Rc`s, property access has to
// deref through all groups on the path while modification has to `make_mut`
// all of them. Immutable and mutable getters are automatically generated
// and the tree structure itself is entirely private.
//
// Also currently the macro only supports one layer but it's not like that's
// too difficult to change.
macros::implement_style_module! {
    rc font {
        #[parse(values::FontFamilies)]
        #[copy(no)] font_family: Rc<[Rc<str>]> = rc_static!([rc_static!(str b"serif")]),
        #[parse(values::FontWeight)]
        font_weight: I16Dot16 = I16Dot16::new(400),
        #[parse(values::FontSize)]
        font_size: I26Dot6 = I26Dot6::new(16),
        font_slant: FontSlant = FontSlant::Regular,
        #[parse(values::FontFeatureSettings)]
        #[copy(no)] font_feature_settings: FontFeatureSettings = FontFeatureSettings::empty(),
    }

    rc text_inherited {
        #[copy(no)] text_shadows: Rc<[TextShadow]> = rc_static!([]),
        line_break: LineBreakStrictness = LineBreakStrictness::Normal,
        word_break: LineBreakWordOption = LineBreakWordOption::Normal,
        text_align: HorizontalAlignment = HorizontalAlignment::Left,
        inline_sizing: InlineSizing = InlineSizing::Normal,
        direction: Direction = Direction::Ltr,
        white_space_collapse: WhiteSpaceCollapse = WhiteSpaceCollapse::Preserve,
    }

    rc uninherited {
        #[parse(values::Color)]
        #[inherit(no)] background_color: BGRA8 = BGRA8::ZERO,
        #[inherit(no)] text_decoration: TextDecorations = TextDecorations::NONE,
        #[inherit(no)] baseline_source: BaselineSource = BaselineSource::Last,
        #[parse(values::Length)]
        #[inherit(no)] padding_top: Length = Length::ZERO,
        #[inherit(no)] padding_left: Length = Length::ZERO,
        #[inherit(no)] padding_right: Length = Length::ZERO,
        #[inherit(no)] padding_bottom: Length = Length::ZERO,
    }

    rc misc {
        #[parse(values::Color)]
        color: BGRA8 = BGRA8::WHITE,
        visibility: Visibility = Visibility::Visible,
    }
}

static PROPERTIES: LazyLock<HashMap<&'static str, values::ParseAndComputeFn>> =
    LazyLock::new(|| {
        let mut result = HashMap::new();
        for &(name, fun) in properties::PARSERS {
            result.insert(name, fun);
        }
        result
    });

pub fn from_declarations(
    log: LogContext,
    declarations: Vec<Declaration<'_>>,
    parent: &ComputedStyle,
) -> ComputedStyle {
    let mut result = parent.create_derived();

    let mut declarations: Vec<_> = declarations
        .into_iter()
        .map(|decl| {
            let mut name = decl.name.value().to_string();
            name.make_ascii_lowercase();
            (name, decl.value, decl.important)
        })
        .collect();
    declarations.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));

    for declaration in declarations {
        let Some(&handler) = PROPERTIES.get(declaration.0.as_str()) else {
            warn!(
                log,
                "Ignoring unrecognized declaration with name {:?}", declaration.0
            );
            continue;
        };

        match handler(&mut result, declaration.1, parent) {
            Ok(()) => (),
            Err(error) => {
                warn!(log, "Failed to parse '{}' value: {}", declaration.0, error);
            }
        }
    }

    result
}

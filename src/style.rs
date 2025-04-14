use std::{collections::HashMap, sync::LazyLock};

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use log::{warn, LogContext};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
    rc_static,
};

pub mod parsers;

pub mod computed;
use computed::*;

use crate::{csssyn::algorithms::Declaration, style::parsers::DeclarationHandler};

trait ComputedProperty {
    type Value: Clone + 'static;
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
        #[copy(no)] font_family: Rc<[Rc<str>]> = rc_static!([rc_static!(str b"serif")]),
        font_weight: I16Dot16 = I16Dot16::new(400),
        font_size: I26Dot6 = I26Dot6::new(16),
        font_slant: FontSlant = FontSlant::Regular,
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
        #[inherit(no)] background_color: Color = Color::TRANSPARENT,
        #[inherit(no)] text_decoration_line: TextDecorationLines = TextDecorationLines::NONE,
        #[inherit(no)] text_decoration_color: Color = Color::CurrentColor,
        #[inherit(no)] baseline_source: BaselineSource = BaselineSource::Last,
        #[inherit(no)] padding_top: Length = Length::ZERO,
        #[inherit(no)] padding_left: Length = Length::ZERO,
        #[inherit(no)] padding_right: Length = Length::ZERO,
        #[inherit(no)] padding_bottom: Length = Length::ZERO,
    }

    rc misc {
        color: BGRA8 = BGRA8::WHITE,
        visibility: Visibility = Visibility::Visible,
    }
}

type DeclarationHandlerMap = HashMap<&'static str, &'static DeclarationHandler>;

static DECLARATION_HANDLER_MAP: LazyLock<DeclarationHandlerMap> = LazyLock::new(|| {
    let mut result = HashMap::new();
    for handler in parsers::DECLARATION_HANDLERS {
        result.insert(handler.name, handler);
    }
    result
});

#[cfg_attr(not(all(test, feature = "_layout_tests")), expect(dead_code))]
pub fn compute_with_declarations(
    log: &LogContext,
    declarations: &mut dyn Iterator<Item = &[Declaration<'_>]>,
    parent: &ComputedStyle,
) -> ComputedStyle {
    let mut result = parent.create_derived();

    let mut name_buffer = String::new();
    let mut declarations: Vec<_> = declarations
        .flatten()
        .filter_map(|decl| {
            name_buffer.clear();
            name_buffer.extend(
                decl.name
                    .value()
                    .unescape_iter()
                    .map(|x| x.to_ascii_lowercase()),
            );

            let Some(handler) = DECLARATION_HANDLER_MAP.get(&*name_buffer) else {
                warn!(log, "Ignoring unrecognized declaration for '{name_buffer}'");
                return None;
            };

            Some((*handler, decl.value, decl.important))
        })
        .collect();
    declarations.sort_by_key(|&(_, _, important)| important);

    for (handler, value, _) in declarations {
        match (handler.parse_and_compute)(&mut result, value, parent) {
            Ok(()) => (),
            Err(error) => {
                warn!(log, "Failed to parse '{}' value: {}", handler.name, error);
            }
        }
    }

    result
}

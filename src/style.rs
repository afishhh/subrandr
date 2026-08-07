use std::{collections::HashMap, sync::LazyLock};

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use log::{log_once_state, warn, LogContext, LogOnceSet};
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
        writing_mode: WritingMode = WritingMode::HorizontalTtb,
        text_orientation: TextOrientation = TextOrientation::Mixed,
        white_space_collapse: WhiteSpaceCollapse = WhiteSpaceCollapse::Preserve,
    }

    rc uninherited {
        #[inherit(no)] background_color: Color = Color::TRANSPARENT,
        #[inherit(no)] text_decoration_line: TextDecorationLines = TextDecorationLines::NONE,
        #[inherit(no)] text_decoration_color: Color = Color::CurrentColor,
        #[inherit(no)] baseline_source: BaselineSource = BaselineSource::Last,
        #[inherit(no)] height: Option<Length> = None,
        #[inherit(no)] width: Option<Length> = None,
        #[inherit(no)] padding_top: Length = Length::ZERO,
        #[inherit(no)] padding_left: Length = Length::ZERO,
        #[inherit(no)] padding_right: Length = Length::ZERO,
        #[inherit(no)] padding_bottom: Length = Length::ZERO,
        #[inherit(no)] margin_top: Option<Length> = Some(Length::ZERO),
        #[inherit(no)] margin_left: Option<Length> = Some(Length::ZERO),
        #[inherit(no)] margin_right: Option<Length> = Some(Length::ZERO),
        #[inherit(no)] margin_bottom: Option<Length> = Some(Length::ZERO),
        #[inherit(no)] transform: Option<Transform> = None,
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

pub struct DeclarationFilter(DeclarationHandlerMap, &'static str);

impl DeclarationFilter {
    pub fn new(allowed: impl IntoIterator<Item = &'static str>, message: &'static str) -> Self {
        let it = allowed.into_iter();
        let mut result = HashMap::with_capacity(it.size_hint().0);

        for name in it {
            result.insert(
                name,
                *DECLARATION_HANDLER_MAP.get(name).unwrap_or_else(|| {
                    unreachable!("unknown name in declaration filter: {name:?}")
                }),
            );
        }

        Self(result, message)
    }
}

pub fn apply_declarations_to(
    log: &LogContext,
    target: ComputedStyle,
    declarations: &mut dyn Iterator<Item = &[Declaration<'_>]>,
    parent: &ComputedStyle,
    filter: Option<&DeclarationFilter>,
    logset: &LogOnceSet,
) -> ComputedStyle {
    log_once_state!(in logset; ignoring_declaration, invalid_declaration_value);

    let handler_map = filter.map_or(&*DECLARATION_HANDLER_MAP, |f| &f.0);
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

            let Some(handler) = handler_map.get(&*name_buffer) else {
                if let Some(filter) =
                    filter.filter(|_| DECLARATION_HANDLER_MAP.contains_key(&*name_buffer))
                {
                    warn!(
                        log,
                        once(ignoring_declaration, &name_buffer),
                        "Ignoring declaration for '{name_buffer}': {}",
                        filter.1
                    );
                } else {
                    warn!(
                        log,
                        once(ignoring_declaration, &name_buffer),
                        "Ignoring unrecognized declaration for '{name_buffer}'"
                    );
                }
                return None;
            };

            Some((*handler, decl.value, decl.important))
        })
        .collect();
    declarations.sort_by_key(|&(_, _, important)| important);

    let mut result = target;
    for (handler, value, _) in declarations {
        match (handler.parse_and_compute)(&mut result, value, parent) {
            Ok(()) => (),
            Err(error) => {
                let error_string = error.to_string();
                warn!(
                    log,
                    once(invalid_declaration_value, (handler.name, &error_string)),
                    "Failed to parse '{}' value: {error_string}",
                    handler.name,
                );
            }
        }
    }

    result
}

#[cfg_attr(not(all(test, feature = "_layout_tests")), expect(dead_code))]
pub fn compute_with_declarations(
    log: &LogContext,
    declarations: &mut dyn Iterator<Item = &[Declaration<'_>]>,
    parent: &ComputedStyle,
) -> ComputedStyle {
    apply_declarations_to(
        log,
        parent.create_derived(),
        declarations,
        parent,
        None,
        &LogOnceSet::new(),
    )
}

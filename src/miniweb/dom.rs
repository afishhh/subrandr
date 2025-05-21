use std::{ops::Range, rc::Rc};

use crate::{
    miniweb::{
        layout,
        style::{
            types::{Display, FullDisplay, InsideDisplayType, OutsideDisplayType, Ruby},
            ComputedStyle, StyleMap,
        },
    },
    symbol::{Symbol, SymbolInterner},
};

#[derive(Debug, Clone)]
pub struct DomObject {
    pub name: Symbol,
    pub id: Option<Symbol>,
    pub classes: Vec<Symbol>,
    pub style: ComputedStyle,
    pub time: u32,
}

impl DomObject {
    fn new(name: Symbol, id: Option<Symbol>, classes: Vec<Symbol>, time: u32) -> Self {
        Self {
            name,
            id,
            classes,
            time,
            style: ComputedStyle::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Element {
    pub object: DomObject,
    pub children: Vec<ElementOrText>,
}

#[derive(Debug, Clone)]
pub enum ElementOrText {
    Element(Box<Element>),
    Text(TextSequence),
}

#[derive(Debug, Clone)]
pub struct TextSequence {
    pub text: Rc<str>,
    pub ruby: Ruby,
}

#[derive(Debug, Clone)]
enum FormattingContextKind {
    Inline,
    Block,
}

impl Element {
    fn make_layout_box(&mut self) -> Option<(FullDisplay, layout::Container)> {
        let style = &self.object.style;

        let display = match style.display() {
            Display::None => return None,
            Display::Full(full_display) => full_display,
        };

        let mut result = match display.inner {
            InsideDisplayType::Flow => {
                if matches!(
                    display.outer,
                    OutsideDisplayType::Inline /* or RunIn */
                ) {
                    layout::Container::Inline(layout::InlineContainer {
                        style: style.clone(),
                        contents: Vec::new(),
                    })
                } else {
                    layout::Container::Block(layout::BlockContainer {
                        style: style.clone(),
                        contents: Vec::new(),
                    })
                }
            }
            InsideDisplayType::FlowRoot => layout::Container::Block(layout::BlockContainer {
                style: style.clone(),
                contents: Vec::new(),
            }),
        };

        for child in &mut self.children {
            match child {
                ElementOrText::Element(element) => {
                    let Some((display, element_box)) = element.make_layout_box() else {
                        continue;
                    };

                    match (&mut result, display.outer) {
                        (layout::Container::Inline(_inline), OutsideDisplayType::Block) => {
                            todo!("atomic inlines")
                        }
                        (layout::Container::Inline(_inline), OutsideDisplayType::Inline) => {
                            todo!("inline boxes in inline boxes")
                        }
                        (layout::Container::Block(block), _) => {
                            block.contents.push(element_box);
                        }
                    }
                }
                ElementOrText::Text(text) => match &mut result {
                    layout::Container::Inline(inline) => inline.contents.push(layout::InlineText {
                        style: style.clone(),
                        text: text.text.clone(),
                        ruby: text.ruby,
                    }),
                    layout::Container::Block(block) => {
                        block
                            .contents
                            .push(layout::Container::Inline(layout::InlineContainer {
                                style: style.clone(),
                                contents: vec![layout::InlineText {
                                    style: style.clone(),
                                    text: text.text.clone(),
                                    ruby: text.ruby,
                                }],
                            }))
                    }
                },
            }
        }

        Some((display, result))
    }
}

pub struct StylingContext {
    pub time: u32,
}

#[derive(Debug, Clone)]
pub struct Selector {
    pub name: Option<Symbol>,
    pub id: Option<Symbol>,
    pub classes: Vec<Symbol>,
    pub time_interval: Range<u32>,
    pub future: bool,
    pub past: bool,
}

impl Default for Selector {
    fn default() -> Self {
        Self {
            name: None,
            id: None,
            classes: Vec::new(),
            time_interval: 0..u32::MAX,
            future: false,
            past: false,
        }
    }
}

impl Selector {
    pub fn matches(&self, ctx: &StylingContext, object: &DomObject) -> bool {
        self.time_interval.contains(&ctx.time)
            && self.name.as_ref().is_none_or(|name| name == &object.name)
            && self
                .id
                .as_ref()
                .is_none_or(|id| Some(id) == object.id.as_ref())
            && self
                .classes
                .iter()
                .all(|class| object.classes.contains(class))
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub specificity: u32,
    pub declarations: StyleMap,
}

impl Rule {
    pub fn for_name(name: Symbol, declarations: StyleMap) -> Self {
        Self {
            selectors: vec![Selector {
                name: Some(name),
                ..Default::default()
            }],
            specificity: 0,
            declarations,
        }
    }

    pub fn matches(&self, ctx: &StylingContext, object: &DomObject) -> bool {
        self.selectors
            .iter()
            .any(|selector| selector.matches(ctx, object))
    }

    pub fn apply_if_matches(&self, ctx: &StylingContext, object: &mut DomObject) {
        if self.matches(ctx, object) {
            object.style.apply_all(&self.declarations);
        }
    }
}

pub struct Document {
    pub root: Element,
    pub style_rules: Vec<Rule>,
}

impl Document {
    pub fn new(interner: &SymbolInterner) -> Self {
        Self {
            root: Element {
                object: DomObject::new(interner.intern("root"), None, Vec::new(), 0),
                children: Vec::new(),
            },
            style_rules: Vec::new(),
        }
    }

    fn recompute_styles_for_element(rules: &[Rule], ctx: &StylingContext, element: &mut Element) {
        for rule in rules {
            rule.apply_if_matches(ctx, &mut element.object);
        }

        for child in &mut element.children {
            match child {
                ElementOrText::Element(element) => {
                    Self::recompute_styles_for_element(rules, ctx, element);
                }
                ElementOrText::Text(_) => (),
            }
        }
    }

    pub fn recompute_styles(&mut self, ctx: &StylingContext) {
        Self::recompute_styles_for_element(&self.style_rules, ctx, &mut self.root);
    }

    pub fn make_layout_tree(&mut self) -> layout::Container {
        self.root.make_layout_box().map_or_else(
            || {
                layout::Container::Block(layout::BlockContainer {
                    style: ComputedStyle::default(),
                    contents: Vec::new(),
                })
            },
            |(_, b)| b,
        )
    }
}

#[cfg(test)]
mod test {
    use crate::miniweb::style;

    use super::*;

    #[test]
    pub fn xd() {
        let symbols = SymbolInterner::new();
        let span = symbols.intern("span");
        let root = symbols.intern("root");

        let mut document = Document::new(&symbols);
        document.root.children.extend([
            ElementOrText::Text(TextSequence {
                text: "hello".into(),
                ruby: Ruby::None,
            }),
            ElementOrText::Element(Box::new(Element {
                object: DomObject::new(span.clone(), None, Vec::new(), 0),
                children: vec![ElementOrText::Text(TextSequence {
                    text: "more text".into(),
                    ruby: Ruby::None,
                })],
            })),
        ]);

        document.style_rules.push(Rule::for_name(root, {
            let mut result = StyleMap::new();
            result.set::<style::Display>(Display::BLOCK);
            result
        }));

        document.style_rules.push(Rule::for_name(span, {
            let mut result = StyleMap::new();
            result.set::<style::Display>(Display::INLINE);
            result
        }));

        document.recompute_styles(&StylingContext { time: 0 });

        dbg!(&document.root);

        dbg!(document.make_layout_tree());
    }
}

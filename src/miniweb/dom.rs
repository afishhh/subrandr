use std::{ops::Range, pin::Pin, rc::Rc};

use crate::miniweb::{
    layout,
    realm::symbol::Symbol,
    style::{
        types::{Display, FullDisplay, InsideDisplayType, OutsideDisplayType, Ruby},
        ComputedStyle, StyleMap,
    },
};

use super::realm::Realm;

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

enum LayoutBox {
    Block(layout::BlockContainer),
    Inline(layout::InlineContainer),
    Ruby(layout::RubyContainer),
}

impl LayoutBox {
    fn blockify(self) -> layout::Container {
        match self {
            LayoutBox::Block(block) => layout::Container::Block(block),
            LayoutBox::Inline(inline) => layout::Container::Inline(inline),
            LayoutBox::Ruby(ruby) => layout::Container::Inline(layout::InlineContainer {
                contents: vec![layout::InlineChild::Ruby(layout::RubyContainer {
                    style: ruby.style.create_child(),
                    contents: ruby.contents,
                })],
                style: ruby.style,
            }),
        }
    }
}

impl Element {
    fn make_flow_inline(&mut self) -> layout::InlineContainer {
        let mut result = layout::InlineContainer {
            style: self.object.style.clone(),
            contents: Vec::new(),
        };

        let child_style = self.object.style.create_child();
        for child in &mut self.children {
            match child {
                ElementOrText::Element(element) => {
                    let Some((display, element_box)) = element.make_layout_box() else {
                        continue;
                    };

                    match display.outer {
                        OutsideDisplayType::Block => {
                            _ = element_box;
                            todo!("atomic inlines")
                        }
                        OutsideDisplayType::Inline => {
                            _ = element_box;
                            todo!("inline boxes in inline boxes")
                        }
                    }
                }
                ElementOrText::Text(text) => {
                    result
                        .contents
                        .push(layout::InlineChild::Text(layout::InlineText {
                            style: child_style.clone(),
                            text: text.text.clone(),
                            ruby: text.ruby,
                        }))
                }
            }
        }

        result
    }

    fn make_flow_block(&mut self) -> layout::BlockContainer {
        let mut result = layout::BlockContainer {
            style: self.object.style.clone(),
            contents: Vec::new(),
        };

        let child_style = self.object.style.create_child();
        for child in &mut self.children {
            match child {
                ElementOrText::Element(element) => {
                    let Some((_, element_box)) = element.make_layout_box() else {
                        continue;
                    };

                    result.contents.push(element_box.blockify());
                }
                ElementOrText::Text(text) => {
                    result
                        .contents
                        .push(layout::Container::Inline(layout::InlineContainer {
                            style: child_style.clone(),
                            contents: vec![layout::InlineChild::Text(layout::InlineText {
                                style: child_style.clone(),
                                text: text.text.clone(),
                                ruby: text.ruby,
                            })],
                        }))
                }
            }
        }

        result
    }

    fn make_layout_box(&mut self) -> Option<(FullDisplay, LayoutBox)> {
        let style = &self.object.style;

        let display = match style.display() {
            Display::None => return None,
            Display::Full(full_display) => full_display,
            Display::Internal(_) => {
                // https://www.w3.org/TR/css-ruby-1/#box-fixup
                todo!()
            }
        };

        let result = match display.inner {
            InsideDisplayType::Flow => {
                if matches!(
                    display.outer,
                    OutsideDisplayType::Inline /* or RunIn */
                ) {
                    LayoutBox::Inline(self.make_flow_inline())
                } else {
                    LayoutBox::Block(self.make_flow_block())
                }
            }
            InsideDisplayType::FlowRoot => LayoutBox::Block(self.make_flow_block()),
            InsideDisplayType::Ruby => {
                let ruby = layout::RubyContainer {
                    style: match display.outer {
                        // Only inheritable properties should propagate to the
                        // block principal box's ruby container child.
                        OutsideDisplayType::Block => style.create_child(),
                        OutsideDisplayType::Inline => style.clone(),
                    },
                    contents: Vec::new(),
                };

                match display.outer {
                    OutsideDisplayType::Block => LayoutBox::Inline(layout::InlineContainer {
                        style: style.clone(),
                        contents: vec![layout::InlineChild::Ruby(ruby)],
                    }),
                    OutsideDisplayType::Inline => LayoutBox::Ruby(ruby),
                }
            }
        };

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
    root: Element,
    realm: Rc<Realm>,
    pub style_rules: Vec<Rule>,
}

impl Document {
    pub fn new(realm: Rc<Realm>) -> Self {
        Self {
            root: Element {
                object: DomObject::new(realm.symbol("root"), None, Vec::new(), 0),
                children: Vec::new(),
            },
            realm,
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

    pub fn root(&mut self) -> Pin<&mut Element> {
        Pin::new(&mut self.root)
    }

    pub fn realm(&self) -> &Rc<Realm> {
        &self.realm
    }

    pub fn make_layout_tree(&mut self) -> layout::Container {
        self.root.make_layout_box().map_or_else(
            || {
                layout::Container::Block(layout::BlockContainer {
                    style: ComputedStyle::default(),
                    contents: Vec::new(),
                })
            },
            |(_, b)| b.blockify(),
        )
    }
}

#[cfg(test)]
mod test {
    use crate::miniweb::style;

    use super::*;

    #[test]
    pub fn xd() {
        let realm = Realm::create();
        let mut document = Document::new(realm.clone());

        let span = realm.symbol("span");
        let root = realm.symbol("root");
        let ruby = realm.symbol("ruby");

        document.root().children.extend([
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

        document.style_rules.push(Rule::for_name(ruby, {
            let mut result = StyleMap::new();
            result.set::<style::Display>(Display::RUBY);
            result
        }));

        document.recompute_styles(&StylingContext { time: 0 });

        dbg!(document.root());

        dbg!(document.make_layout_tree());
    }
}

// https://www.w3.org/TR/css-ruby-1/#default-ua-ruby
// ruby { display: ruby; }
// rp   { display: none; }
// rbc  { display: ruby-base-container; }
// rtc  { display: ruby-text-container; }
// rb   { display: ruby-base; white-space: nowrap; }
// rt   { display: ruby-text; }
// ruby, rb, rt, rbc, rtc { unicode-bidi: isolate; }

// rtc, rt {
//   font-variant-east-asian: ruby;  /* See [[CSS-FONTS-3]] */
//   text-justify: ruby;             /* See [[CSS-TEXT-4]] */
//   text-emphasis: none;            /* See [[CSS-TEXT-DECOR-3]] */
//   white-space: nowrap;
//   line-height: 1; }

// rtc, :not(rtc) > rt {
//   font-size: 50%;
// }
// rtc:lang(zh-TW), :not(rtc) > rt:lang(zh-TW),
// rtc:lang(zh-Hanb), :not(rtc) > rt:lang(zh-Hanb), {
//   font-size: 30%;                /* bopomofo */
// }

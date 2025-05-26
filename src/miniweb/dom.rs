use std::rc::Rc;

use crate::miniweb::{
    layout,
    realm::symbol::Symbol,
    style::{
        computed::{
            Display, FullDisplay, InsideDisplayType, InternalDisplay, OutsideDisplayType, Ruby,
        },
        ComputedStyle,
    },
};

use super::{
    realm::Realm,
    style::{
        restyle::{StylesheetIndex, StylingContext},
        sheet::{selector, Origin, Rule, Stylesheet},
        style_map,
    },
};

#[derive(Debug, Clone)]
pub struct DomElement {
    pub name: Symbol,
    pub id: Option<Symbol>,
    pub classes: Vec<Symbol>,
    pub style: ComputedStyle,
    pub time: u32,
}

impl DomElement {
    pub fn new(name: Symbol, id: Option<Symbol>, classes: Vec<Symbol>, time: u32) -> Self {
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
    pub object: DomElement,
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

pub struct Document {
    root: Element,
    realm: Rc<Realm>,
    styles: StylesheetIndex,
}

impl Document {
    pub fn new(realm: Rc<Realm>) -> Self {
        Self {
            root: Element {
                object: DomElement::new(realm.symbol("root"), None, Vec::new(), 0),
                children: Vec::new(),
            },
            realm,
            styles: StylesheetIndex::new(),
        }
    }

    pub fn root(&mut self) -> &mut Element {
        &mut self.root
    }

    pub fn realm(&self) -> &Rc<Realm> {
        &self.realm
    }

    pub fn add_stylesheet(&mut self, sheet: Stylesheet) {
        self.styles.add_stylesheet(sheet);
    }

    pub fn restyle(&mut self, ctx: &StylingContext) {
        self.styles.restyle(ctx, &mut self.root);
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
    use crate::miniweb::{layout::Vec2L, style::restyle::StylingContext};

    use super::*;

    #[test]
    pub fn does_not_crash() {
        let realm = Realm::create();
        let mut document = Document::new(realm.clone());

        document.root().children.extend([
            ElementOrText::Text(TextSequence {
                text: "hello".into(),
                ruby: Ruby::None,
            }),
            ElementOrText::Element(Box::new(Element {
                object: DomElement::new(realm.symbol("span"), None, Vec::new(), 0),
                children: vec![ElementOrText::Text(TextSequence {
                    text: "more text".into(),
                    ruby: Ruby::None,
                })],
            })),
        ]);

        let mut stylesheet = Stylesheet::new(Origin::UserAgent);

        stylesheet.rules.push(Rule::new(
            vec![selector!(in realm; "root")],
            style_map! {
                Display: Display::BLOCK;
            },
        ));

        stylesheet.rules.push(Rule::new(
            vec![selector!(in realm; "span")],
            style_map! {
                Display: Display::INLINE;
            },
        ));

        document.add_stylesheet(stylesheet);
        document.add_stylesheet(ruby_ua_stylesheet(&realm));

        document.restyle(&StylingContext {
            time: 0,
            viewport_size: Vec2L::ZERO,
        });

        dbg!(document.root());
        dbg!(document.make_layout_tree());
    }
}

// https://www.w3.org/TR/css-ruby-1/#default-ua-ruby
pub fn ruby_ua_stylesheet(realm: &Realm) -> Stylesheet {
    let mut stylesheet = Stylesheet::new(Origin::UserAgent);

    // ruby { display: ruby; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "ruby")],
        style_map!(
            Display: Display::RUBY;
        ),
    ));

    // rp   { display: none; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rp")],
        style_map!(
            Display: Display::NONE;
        ),
    ));

    // rbc  { display: ruby-base-container; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rbc")],
        style_map!(
            Display: Display::Internal(InternalDisplay::RubyBaseContainer);
        ),
    ));

    // rtc  { display: ruby-text-container; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rtc")],
        style_map!(
            Display: Display::Internal(InternalDisplay::RubyTextContainer);
        ),
    ));

    // rb   { display: ruby-base; white-space: nowrap; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rb")],
        style_map!(
            Display: Display::Internal(InternalDisplay::RubyBase);
            // TODO: white-space: nowrap;
        ),
    ));

    // rt   { display: ruby-text; }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rt")],
        style_map!(
            Display: Display::Internal(InternalDisplay::RubyText);
        ),
    ));

    // ruby, rb, rt, rbc, rtc { unicode-bidi: isolate; }
    stylesheet.rules.push(Rule::new(
        vec![
            selector!(in realm; "ruby"),
            selector!(in realm; "rb"),
            selector!(in realm; "rt"),
            selector!(in realm; "rbc"),
            selector!(in realm; "rtc"),
        ],
        style_map!(
            // unicode-bidi: isolate
        ),
    ));

    // rtc, rt {
    //   font-variant-east-asian: ruby;  /* See [[CSS-FONTS-3]] */
    //   text-justify: ruby;             /* See [[CSS-TEXT-4]] */
    //   text-emphasis: none;            /* See [[CSS-TEXT-DECOR-3]] */
    //   white-space: nowrap;
    //   line-height: 1;
    // }
    stylesheet.rules.push(Rule::new(
        vec![selector!(in realm; "rtc"), selector!(in realm; "rt")],
        style_map!(
            // TODO: font-variant-east-asian: ruby;  /* See [[CSS-FONTS-3]] */
            // TODO: text-justify: ruby;
            // TODO: text-emphasis: none;
            // TODO: white-space: nowrap;
            // TODO: line-height: 1;
        ),
    ));

    // rtc, :not(rtc) > rt {
    //   font-size: 50%;
    // }
    stylesheet.rules.push(Rule::new(
        // FIXME: should be rtc, :not(rtc) > rt
        vec![selector!(in realm; "rt")],
        style_map!(
            // TODO: font-size: 50%;
        ),
    ));

    // TODO:
    // rtc:lang(zh-TW), :not(rtc) > rt:lang(zh-TW),
    // rtc:lang(zh-Hanb), :not(rtc) > rt:lang(zh-Hanb), {
    //   font-size: 30%;                /* bopomofo */
    // }

    stylesheet
}

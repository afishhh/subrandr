use std::borrow::Cow;

mod tokenizer;

use tokenizer::CueTextTokenizer;
pub use tokenizer::{Annotation, ClassList, Text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalNodeKind<'a> {
    Class,
    Italic,
    Bold,
    Underline,
    Ruby,
    RubyText,
    Voice { value: Annotation<'a> },
    Language,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalNode<'a> {
    pub kind: InternalNodeKind<'a>,
    pub classes: ClassList<'a>,
    pub language: Option<Cow<'a, str>>,
    pub children: Vec<Node<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'a> {
    Internal(InternalNode<'a>),
    Text(Text<'a>),
    Timestamp(u32),
}

pub(crate) fn parse_cue_text(input: &str) -> Vec<Node<'_>> {
    let mut tokenizer = CueTextTokenizer::new(input);

    let mut node_info_stack: Vec<(InternalNodeKind, ClassList, Option<Cow<str>>)> = vec![];
    let mut children_stack: Vec<Vec<Node>> = vec![Vec::new()];
    let mut language_stack: Vec<Cow<str>> = Vec::new();

    fn exit_node<'a>(
        info_stack: &mut Vec<(InternalNodeKind<'a>, ClassList<'a>, Option<Cow<'a, str>>)>,
        children_stack: &mut Vec<Vec<Node<'a>>>,
    ) {
        let info = info_stack.pop().unwrap();
        let children = children_stack.pop().unwrap();
        let current = children_stack.last_mut().unwrap();
        current.push(Node::Internal(InternalNode {
            kind: info.0,
            classes: info.1,
            language: info.2,
            children,
        }));
    }

    while let Some(token) = tokenizer.next() {
        let current = children_stack.last_mut().unwrap();
        match token {
            tokenizer::Token::Text(text) => current.push(Node::Text(text)),
            tokenizer::Token::StartTag(start_tag) => {
                let kind = match start_tag.name {
                    "c" => InternalNodeKind::Class,
                    "i" => InternalNodeKind::Italic,
                    "b" => InternalNodeKind::Bold,
                    "u" => InternalNodeKind::Underline,
                    "ruby" => InternalNodeKind::Ruby,
                    "rt" if node_info_stack
                        .last()
                        .is_some_and(|(kind, ..)| matches!(kind, InternalNodeKind::Ruby)) =>
                    {
                        InternalNodeKind::RubyText
                    }
                    "v" => InternalNodeKind::Voice {
                        value: start_tag.annotation.unwrap_or_default(),
                    },
                    "lang" => {
                        language_stack.push(start_tag.annotation.unwrap_or_default().content());
                        InternalNodeKind::Language
                    }
                    _ => continue,
                };
                node_info_stack.push((kind, start_tag.classes, language_stack.last().cloned()));
                children_stack.push(Vec::new());
            }
            tokenizer::Token::EndTag(end_tag) => {
                match (end_tag, node_info_stack.last().map(|(kind, ..)| kind)) {
                    ("c", Some(InternalNodeKind::Class))
                    | ("i", Some(InternalNodeKind::Italic))
                    | ("b", Some(InternalNodeKind::Bold))
                    | ("u", Some(InternalNodeKind::Underline))
                    | ("ruby", Some(InternalNodeKind::Ruby))
                    | ("rt", Some(InternalNodeKind::RubyText))
                    | ("v", Some(InternalNodeKind::Voice { .. })) => {
                        exit_node(&mut node_info_stack, &mut children_stack);
                    }
                    ("lang", Some(InternalNodeKind::Language)) => {
                        exit_node(&mut node_info_stack, &mut children_stack);
                        _ = language_stack.pop();
                    }
                    ("ruby", Some(InternalNodeKind::RubyText)) => {
                        exit_node(&mut node_info_stack, &mut children_stack);
                        exit_node(&mut node_info_stack, &mut children_stack);
                    }
                    _ => continue,
                }
            }
            tokenizer::Token::TimestampTag(timestamp) => {
                let mut buffer = super::ParsingBuffer::new(timestamp);
                if let Some(value) =
                    super::collect_timestamp(&mut buffer).filter(|_| buffer.is_empty())
                {
                    current.push(Node::Timestamp(value))
                }
            }
        }
    }

    while !node_info_stack.is_empty() {
        exit_node(&mut node_info_stack, &mut children_stack);
    }

    assert_eq!(children_stack.len(), 1);

    children_stack.pop().unwrap()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn example_22_class_in_cue() {
        let list = parse_cue_text(
            r#"
<c.loud>Yellow!</c>
<i.loud>Yellow!</i>
<u.loud>Yellow!</u>
<b.loud>Yellow!</b>
<u.loud>Yellow!</u>
<ruby.loud>Yellow! <rt.loud>Yellow!</rt></ruby>
<v.very.loud Kathryn>Yellow!</v>
<lang.loud en>Yellow!"#
                .trim(),
        );

        assert_eq!(
            &list,
            &[
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Class,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Italic,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Underline,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Bold,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Underline,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Ruby,
                    classes: ClassList::new("loud"),
                    language: None,
                    children: vec![
                        Node::Text(Text("Yellow! ")),
                        Node::Internal(InternalNode {
                            kind: InternalNodeKind::RubyText,
                            classes: ClassList::new("loud"),
                            language: None,
                            children: vec![Node::Text(Text("Yellow!"))]
                        })
                    ]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Voice {
                        value: Annotation("Kathryn")
                    },
                    classes: ClassList::new("very.loud"),
                    language: None,
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Language,
                    classes: ClassList::new("loud"),
                    language: Some(Cow::Borrowed("en")),
                    children: vec![Node::Text(Text("Yellow!"))]
                }),
            ]
        )
    }

    #[test]
    fn ruby_after_class() {
        let list = parse_cue_text(
            r#"
<c.red>some red text </c>
<ruby>preceeding ruby<rt>with an annotation</ruby>
"#
            .trim(),
        );

        assert_eq!(
            &list,
            &[
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Class,
                    classes: ClassList::new("red"),
                    language: None,
                    children: vec![Node::Text(Text("some red text "))]
                }),
                Node::Text(Text("\n")),
                Node::Internal(InternalNode {
                    kind: InternalNodeKind::Ruby,
                    classes: ClassList::new(""),
                    language: None,
                    children: vec![
                        Node::Text(Text("preceeding ruby")),
                        Node::Internal(InternalNode {
                            kind: InternalNodeKind::RubyText,
                            classes: ClassList::new(""),
                            language: None,
                            children: vec![Node::Text(Text("with an annotation"))]
                        }),
                    ]
                }),
            ]
        )
    }
}

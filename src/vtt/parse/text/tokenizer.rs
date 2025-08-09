//! The cue text tokenizer as described by the WebVTT specification [here]
//!
//! [here]: https://www.w3.org/TR/webvtt1/#webvtt-cue-text-tokenizer

// TODO: Maybe also make class splitting lazy?
//       This would also mean that there would be a predictable number of
//       buffers in use at any given time depending on the state and the
//       Vec would be completely unnecessary.

use std::borrow::Cow;

use crate::html;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenizerState {
    Data = 0,
    Tag = 1,
    StartTag = 2,
    StartTagClass = 3,
    StartTagAnnotation = 4,
    EndTag = 5,
    TimestampTag = 6,
}

pub struct CueTextTokenizer<'a> {
    input: &'a str,
    buffers: [&'a str; 3],
    position: usize,
}

impl<'a> CueTextTokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            buffers: [""; 3],
            position: 0,
        }
    }

    fn step(&mut self) -> TokenizerState {
        let mut state = TokenizerState::Data;
        let mut start = self.position;

        loop {
            let byte = self.input.as_bytes().get(self.position);

            match state {
                TokenizerState::Data => match byte {
                    Some(b'<') => {
                        if start == self.position {
                            self.position += 1;
                            state = TokenizerState::Tag;
                        } else {
                            self.buffers[0] = &self.input[start..self.position];
                            return TokenizerState::Data;
                        }
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                    None => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::Data;
                    }
                },
                TokenizerState::Tag => match byte {
                    Some(b'\t' | b'\n' | b'\x0C' | b' ') => {
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::StartTagAnnotation;
                    }
                    Some(b'.') => {
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::StartTagClass;
                    }
                    Some(b'/') => {
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::EndTag;
                    }
                    Some(b'0'..=b'9') => {
                        start = self.position;
                        self.position += 1;
                        state = TokenizerState::TimestampTag;
                    }
                    Some(b'>') | None => {
                        self.buffers[0] = "";
                        self.position += 1;
                        return TokenizerState::Tag;
                    }
                    Some(_) => {
                        start = self.position;
                        state = TokenizerState::StartTag;
                    }
                },
                TokenizerState::StartTag => match byte {
                    Some(b'\t' | b'\x0C' | b' ') => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.buffers[1] = "";
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::StartTagAnnotation;
                    }
                    // TODO: Why does the spec say to "Set buffer to c" here?
                    //       buffer seems to be trimmed on exit from the annotation state
                    //       so this doesn't seem to make a difference?
                    Some(b'\n') => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.buffers[1] = "";
                        start = self.position;
                        self.position += 1;
                        state = TokenizerState::StartTagAnnotation;
                    }
                    Some(b'.') => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::StartTagClass;
                    }
                    Some(b'>') | None => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::StartTag;
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                },
                TokenizerState::StartTagClass => match byte {
                    Some(b'\t' | b'\x0C' | b' ') => {
                        self.buffers[1] = &self.input[start..self.position];
                        self.position += 1;
                        start = self.position;
                        state = TokenizerState::StartTagAnnotation;
                    }
                    // TODO: See above
                    Some(b'\n') => {
                        self.buffers[1] = &self.input[start..self.position];
                        start = self.position;
                        self.position += 1;
                        state = TokenizerState::StartTagAnnotation;
                    }
                    Some(b'>') | None => {
                        self.buffers[1] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::StartTagClass;
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                },
                TokenizerState::StartTagAnnotation => match byte {
                    Some(b'>') | None => {
                        self.buffers[2] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::StartTagAnnotation;
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                },
                TokenizerState::EndTag => match byte {
                    Some(b'>') | None => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::EndTag;
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                },
                TokenizerState::TimestampTag => match byte {
                    Some(b'>') | None => {
                        self.buffers[0] = &self.input[start..self.position];
                        self.position += 1;
                        return TokenizerState::TimestampTag;
                    }
                    Some(_) => {
                        self.position += 1;
                    }
                },
            }
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Text<'a>(pub(super) &'a str);

impl<'a> Text<'a> {
    pub fn raw_content(&self) -> &'a str {
        self.0
    }

    pub fn content(&self) -> Cow<'a, str> {
        html::unescape(self.raw_content())
            .map_or_else(|| Cow::Borrowed(self.raw_content()), Cow::Owned)
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Annotation<'a>(pub(super) &'a str);

impl<'a> Annotation<'a> {
    pub fn raw_content(&self) -> &'a str {
        self.0
    }

    pub fn content(&self) -> Cow<'a, str> {
        // TODO: What does the standard mean by "with additional allowed characters being '>'"?
        html::unescape(self.raw_content())
            .map_or_else(|| Cow::Borrowed(self.raw_content()), Cow::Owned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ClassList<'a>(&'a str);

impl<'a> ClassList<'a> {
    pub fn new(value: &'a str) -> Self {
        Self(value)
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a str> {
        self.0.split('.').filter(|class| !class.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    Text(Text<'a>),
    StartTag(StartTagToken<'a>),
    EndTag(&'a str),
    TimestampTag(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartTagToken<'a> {
    pub name: &'a str,
    pub classes: ClassList<'a>,
    pub annotation: Option<Annotation<'a>>,
}

impl<'a> CueTextTokenizer<'a> {
    pub fn next<'t>(&'t mut self) -> Option<Token<'a>> {
        if self.position >= self.input.len() {
            return None;
        }

        Some(match self.step() {
            TokenizerState::Data => Token::Text(Text(self.buffers[0])),
            TokenizerState::Tag | TokenizerState::StartTag => Token::StartTag(StartTagToken {
                name: self.buffers[0],
                classes: ClassList::new(""),
                annotation: None,
            }),
            TokenizerState::StartTagClass => Token::StartTag(StartTagToken {
                name: self.buffers[0],
                classes: ClassList(self.buffers[1]),
                annotation: None,
            }),
            TokenizerState::StartTagAnnotation => Token::StartTag(StartTagToken {
                name: self.buffers[0],
                classes: ClassList(self.buffers[1]),
                annotation: Some(Annotation(self.buffers[2].trim_ascii())),
            }),
            TokenizerState::EndTag => Token::EndTag(self.buffers[0]),
            TokenizerState::TimestampTag => Token::TimestampTag(self.buffers[0]),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn check_tokenizer_steps(text: &str, steps: &[(TokenizerState, &[&str])]) {
        let mut tokenizer = CueTextTokenizer::new(text);
        for &(expected_state, buffers) in steps {
            assert!(tokenizer.position <= text.len());
            assert_eq!(tokenizer.step(), expected_state);
            assert_eq!(&tokenizer.buffers[..buffers.len()], buffers);
        }
        assert!(tokenizer.position >= text.len());
    }

    #[test]
    fn step_simple() {
        check_tokenizer_steps(
            "this is a very boring cue",
            &[(TokenizerState::Data, &["this is a very boring cue"])],
        );
    }

    #[test]
    // From example 3 in the spec
    fn step_example_3() {
        check_tokenizer_steps(
            "Hello <b>world</b>!",
            &[
                (TokenizerState::Data, &["Hello "]),
                (TokenizerState::StartTag, &["b"]),
                (TokenizerState::Data, &["world"]),
                (TokenizerState::EndTag, &["b"]),
                (TokenizerState::Data, &["!"]),
            ],
        );
    }

    #[test]
    // From example 5 in the spec
    fn step_example_5() {
        check_tokenizer_steps(
            "Sur les <i.foreignphrase><lang en>playground</lang></i>, ici à Montpellier",
            &[
                (TokenizerState::Data, &["Sur les "]),
                (TokenizerState::StartTagClass, &["i", "foreignphrase"]),
                (TokenizerState::StartTagAnnotation, &["lang", "", "en"]),
                (TokenizerState::Data, &["playground"]),
                (TokenizerState::EndTag, &["lang"]),
                (TokenizerState::EndTag, &["i"]),
                (TokenizerState::Data, &[", ici à Montpellier"]),
            ],
        );
    }

    #[test]
    // From example 6 in the spec
    fn step_example_6_1() {
        check_tokenizer_steps(
            "<v.first.loud Esme>It’s a blue apple tree!",
            &[
                (
                    TokenizerState::StartTagAnnotation,
                    &["v", "first.loud", "Esme"],
                ),
                (TokenizerState::Data, &["It’s a blue apple tree!"]),
            ],
        );
    }

    #[test]
    fn step_example_6_2() {
        check_tokenizer_steps(
            "<v Mary>No way!",
            &[
                (TokenizerState::StartTagAnnotation, &["v", "", "Mary"]),
                (TokenizerState::Data, &["No way!"]),
            ],
        );
    }

    #[test]
    fn step_example_6_3() {
        check_tokenizer_steps(
            "<v Esme>Hee!</v> <i>laughter</i>",
            &[
                (TokenizerState::StartTagAnnotation, &["v", "", "Esme"]),
                (TokenizerState::Data, &["Hee!"]),
                (TokenizerState::EndTag, &["v"]),
                (TokenizerState::Data, &[" "]),
                (TokenizerState::StartTag, &["i"]),
                (TokenizerState::Data, &["laughter"]),
                (TokenizerState::EndTag, &["i"]),
            ],
        );
    }

    #[test]
    fn step_edge_cases() {
        check_tokenizer_steps(
            "<b>hi<",
            &[
                (TokenizerState::StartTag, &["b"]),
                (TokenizerState::Data, &["hi"]),
                (TokenizerState::Tag, &[""]),
            ],
        );
    }

    fn check_tokenizer_tokens(text: &str, tokens: &[Token]) {
        let mut tokenizer = CueTextTokenizer::new(text);
        for &expected in tokens {
            assert_eq!(tokenizer.next(), Some(expected));
        }
        assert_eq!(tokenizer.next(), None);
    }

    impl<'a> StartTagToken<'a> {
        fn new(name: &'a str, classes: &'a str, annotation: Option<&'a str>) -> Self {
            Self {
                name,
                classes: ClassList(classes),
                annotation: annotation.map(Annotation),
            }
        }

        fn simple(name: &'a str) -> Self {
            Self::new(name, "", None)
        }
    }

    impl<'a> Token<'a> {
        fn text(content: &'a str) -> Self {
            Self::Text(Text(content))
        }
    }

    #[test]
    fn tokens_simple() {
        check_tokenizer_tokens(
            "this is a very boring cue",
            &[Token::text("this is a very boring cue")],
        );
    }

    #[test]
    // From example 3 in the spec
    fn tokens_example_3() {
        check_tokenizer_tokens(
            "Hello <b>world</b>!",
            &[
                Token::text("Hello "),
                Token::StartTag(StartTagToken::simple("b")),
                Token::text("world"),
                Token::EndTag("b"),
                Token::text("!"),
            ],
        );
    }

    #[test]
    // From example 5 in the spec
    fn tokens_example_5() {
        check_tokenizer_tokens(
            "Sur les <i.foreignphrase><lang en>playground</lang></i>, ici à Montpellier",
            &[
                Token::text("Sur les "),
                Token::StartTag(StartTagToken::new("i", "foreignphrase", None)),
                Token::StartTag(StartTagToken::new("lang", "", Some("en"))),
                Token::text("playground"),
                Token::EndTag("lang"),
                Token::EndTag("i"),
                Token::text(", ici à Montpellier"),
            ],
        );
    }

    #[test]
    // From example 6 in the spec
    fn tokens_example_6() {
        check_tokenizer_tokens(
            "<v.first.loud Esme>It’s a blue apple tree!",
            &[
                Token::StartTag(StartTagToken::new("v", "first.loud", Some("Esme"))),
                Token::text("It’s a blue apple tree!"),
            ],
        );

        check_tokenizer_tokens(
            "<v Mary>No way!",
            &[
                Token::StartTag(StartTagToken::new("v", "", Some("Mary"))),
                Token::text("No way!"),
            ],
        );

        check_tokenizer_tokens(
            "<v Esme>Hee!</v> <i>laughter</i>",
            &[
                Token::StartTag(StartTagToken::new("v", "", Some("Esme"))),
                Token::text("Hee!"),
                Token::EndTag("v"),
                Token::text(" "),
                Token::StartTag(StartTagToken::simple("i")),
                Token::text("laughter"),
                Token::EndTag("i"),
            ],
        );
    }

    #[test]
    // From example 22 in the spec, specifically the section about :past and :future
    fn tokens_example_22_past_future() {
        #[rustfmt::skip]
        check_tokenizer_tokens(
            r#"
<00:00:16.000> <c>This</c>
<00:00:18.000> <c>can</c>
<00:00:20.000> <c>match</c>
<00:00:22.000> <c>:past/:future</c>
<00:00:24.000>"#
                .trim(),
            &[
                Token::TimestampTag("00:00:16.000"),
                Token::text(" "),
                Token::StartTag(StartTagToken::simple("c")),
                Token::text("This"),
                Token::EndTag("c"),

                Token::text("\n"),

                Token::TimestampTag("00:00:18.000"),
                Token::text(" "),
                Token::StartTag(StartTagToken::simple("c")),
                Token::text("can"),
                Token::EndTag("c"),

                Token::text("\n"),

                Token::TimestampTag("00:00:20.000"),
                Token::text(" "),
                Token::StartTag(StartTagToken::simple("c")),
                Token::text("match"),
                Token::EndTag("c"),

                Token::text("\n"),

                Token::TimestampTag("00:00:22.000"),
                Token::text(" "),
                Token::StartTag(StartTagToken::simple("c")),
                Token::text(":past/:future"),
                Token::EndTag("c"),

                Token::text("\n"),

                Token::TimestampTag("00:00:24.000"),
            ],
        );
    }

    #[test]
    fn tokens_edge_cases() {
        check_tokenizer_tokens(
            "<b>hi<",
            &[
                (Token::StartTag(StartTagToken::simple("b"))),
                (Token::text("hi")),
                (Token::StartTag(StartTagToken::simple(""))),
            ],
        );
    }

    #[test]
    fn tokens_ruby_after_class() {
        check_tokenizer_tokens(
            r#"
<c.red>some red text </c>
<ruby>preceeding ruby<rt>with an annotation</ruby>
"#
            .trim(),
            &[
                (Token::StartTag(StartTagToken::new("c", "red", None))),
                (Token::text("some red text ")),
                (Token::EndTag("c")),
                (Token::text("\n")),
                (Token::StartTag(StartTagToken::simple("ruby"))),
                (Token::text("preceeding ruby")),
                (Token::StartTag(StartTagToken::simple("rt"))),
                (Token::text("with an annotation")),
                (Token::EndTag("ruby")),
            ],
        );
    }
}

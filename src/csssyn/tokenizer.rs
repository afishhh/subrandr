//! https://www.w3.org/TR/css-syntax-3/#tokenization

use std::{fmt::Write, ops::Range};

use thiserror::Error;

use crate::csssyn::Span;

// https://www.w3.org/TR/css-syntax-3/#whitespace
fn is_byte_whitespace(codepoint: u8) -> bool {
    matches!(codepoint, b'\n' | b'\t' | b' ')
}

fn is_whitespace(codepoint: char) -> bool {
    is_byte_whitespace(codepoint as u8)
}

// https://www.w3.org/TR/css-syntax-3/#non-printable-code-point
fn is_non_printable(codepoint: char) -> bool {
    matches!(codepoint, '\0'..='\x08' | '\x0b' | '\x0e'..='\x1f' | '\x7f')
}

// https://www.w3.org/TR/css-syntax-3/#ident-start-code-point
fn is_ident_start(codepoint: char) -> bool {
    matches!(codepoint, 'a'..='z' | 'A'..='Z' | '_') || !codepoint.is_ascii()
}

// https://www.w3.org/TR/css-syntax-3/#ident-code-point
fn is_ident(codepoint: char) -> bool {
    is_ident_start(codepoint) || matches!(codepoint, '0'..='9' | '-')
}

// https://www.w3.org/TR/css-syntax-3/#check-if-two-code-points-are-a-valid-escape
fn is_valid_escape(a: char, b: char) -> bool {
    a == '\\' && b != '\n'
}

struct Tokenizer<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            source: text,
            index: 0,
        }
    }

    fn advance_by(&mut self, count_bytes: usize) {
        self.index += count_bytes;
    }

    fn peek_codepoint_inner(&mut self) -> Option<(char, usize)> {
        let mut chrs = self.source[self.index..].char_indices();
        let (_, raw) = chrs.next()?;
        let processed = match raw {
            '\0' => char::REPLACEMENT_CHARACTER,
            '\x0C' => '\n',
            '\r' => {
                let mut tmp = chrs.clone();
                if matches!(tmp.next(), Some((_, '\n'))) {
                    chrs = tmp;
                }
                '\n'
            }
            c => c,
        };
        Some((processed, chrs.offset()))
    }

    fn peek_codepoint(&mut self) -> Option<char> {
        self.peek_codepoint_inner().map(|(c, _)| c)
    }

    fn consume_codepoint(&mut self) -> Option<char> {
        if let Some((chr, len)) = self.peek_codepoint_inner() {
            self.index += len;
            Some(chr)
        } else {
            None
        }
    }

    fn reconsume(&mut self, codepoint: char) {
        let bytes = &self.source.as_bytes()[..self.index];
        if bytes.ends_with(b"\0") {
            self.index -= 1;
        } else if bytes.ends_with(b"\r\n") {
            self.index -= 2;
        } else {
            self.index -= codepoint.len_utf8();
        }
    }

    fn peek_bytes(&self, count: usize) -> &[u8] {
        &self.source.as_bytes()[self.index..(self.index + count).min(self.source.len())]
    }

    fn peek_literal(&self, bytes: &[u8]) -> bool {
        self.source.as_bytes()[self.index..].starts_with(bytes)
    }

    fn find_literal(&self, literal: &str) -> Option<usize> {
        self.source[self.index..]
            .find(literal)
            .map(|v| self.index + v)
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-comment
    fn consume_comments(&mut self) {
        while self.peek_literal(b"/*") {
            self.index = self.find_literal("*/").map_or(self.source.len(), |v| v + 2);
        }
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-string-token
    fn consume_string(&mut self, ending: char) -> TokenKind {
        loop {
            match self.consume_codepoint() {
                Some(c) if c == ending => break,
                None => break,
                Some('\n') => {
                    self.reconsume('\n');
                    return TokenKind::BadString;
                }
                Some('\\') => match self.peek_codepoint() {
                    None => (),
                    Some('\n') => self.advance_by(1),
                    Some(_) => self.skip_escaped_codepoint(),
                },
                Some(_) => {}
            }
        }

        TokenKind::String
    }

    fn peek_is_valid_escape(&mut self, current: char) -> bool {
        self.peek_codepoint()
            .is_some_and(|next| is_valid_escape(current, next))
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-an-ident-sequence
    fn consume_ident_sequence(&mut self) -> Escaped<'a> {
        let start = self.index;

        loop {
            match self.consume_codepoint() {
                Some(c) if is_ident(c) => {}
                Some(c) if self.peek_is_valid_escape(c) => self.skip_escaped_codepoint(),
                Some(c) => {
                    self.reconsume(c);
                    break;
                }
                None => break,
            }
        }

        Escaped(&self.source[start..self.index])
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-number
    fn consume_number(&mut self) -> bool {
        let mut is_integer = true;

        if matches!(self.peek_codepoint(), Some('+' | '-')) {
            self.advance_by(1);
        }

        while matches!(self.peek_codepoint(), Some('0'..='9')) {
            self.advance_by(1);
        }

        let peek = self.peek_bytes(2);
        if matches!(peek.first(), Some(b'.')) && matches!(peek.get(1), Some(b'0'..=b'9')) {
            self.advance_by(2);
            is_integer = false;

            while matches!(self.peek_codepoint(), Some('0'..='9')) {
                self.advance_by(1);
            }
        }

        let peek = self.peek_bytes(3);
        let mut i = 1;
        if matches!(peek.first(), Some(b'E' | b'e')) && {
            i += matches!(peek.get(1), Some(b'+' | b'-')) as usize;
            matches!(peek.get(i), Some(b'0'..=b'9'))
        } {
            self.advance_by(i + 1);
            is_integer = false;

            while matches!(self.peek_codepoint(), Some('0'..='9')) {
                self.advance_by(1);
            }
        }

        is_integer
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-a-numeric-token
    fn consume_numeric_token(&mut self) -> TokenKind {
        let start = self.index;
        let integer = self.consume_number();

        if self.peek_would_start_an_ident_sequence() {
            let unit_offset = (self.index - start) as u32;
            self.consume_ident_sequence();
            TokenKind::Dimension {
                integer,
                unit_offset,
            }
        } else if self.peek_codepoint().is_some_and(|c| c == '%') {
            self.advance_by(1);
            TokenKind::Percentage { integer }
        } else {
            TokenKind::Number { integer }
        }
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-the-remnants-of-a-bad-url
    fn consume_remnants_of_bad_url(&mut self) {
        match self.consume_codepoint() {
            Some(')') | None => (),
            Some('\\') => {
                if self.peek_is_valid_escape('\\') {
                    self.skip_escaped_codepoint();
                }
            }
            Some(_) => (),
        }
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-url-token
    fn consume_unqouted_url(&mut self) -> Option<Range<usize>> {
        while self.peek_codepoint().is_some_and(is_whitespace) {
            self.advance_by(1);
        }

        let start = self.index;
        let end;
        loop {
            match self.consume_codepoint() {
                Some(')') => {
                    end = self.index - 1;
                    break;
                }
                None => {
                    end = self.index;
                    break;
                }
                Some(c) if is_whitespace(c) => {
                    end = self.index - 1;

                    while self.peek_codepoint().is_some_and(is_whitespace) {
                        self.advance_by(1);
                    }

                    if self.peek_codepoint().is_none_or(|c| c == ')') {
                        self.advance_by(1);
                        break;
                    } else {
                        self.consume_remnants_of_bad_url();
                        return None;
                    }
                }
                Some('"' | '\'' | '(') => {
                    self.consume_remnants_of_bad_url();
                    return None;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_remnants_of_bad_url();
                    return None;
                }
                Some('\\') => {
                    if self.peek_is_valid_escape('\\') {
                        self.skip_escaped_codepoint();
                    } else {
                        self.consume_remnants_of_bad_url();
                        return None;
                    }
                }
                Some(_) => {}
            }
        }

        Some(start..end)
    }

    fn lookahead<T>(&mut self, callback: impl FnOnce(&mut Self) -> T) -> T {
        let old = self.index;
        let result = callback(self);
        self.index = old;
        result
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-an-ident-like-token
    fn consume_ident_like(&mut self) -> Result<TokenKind, TokenizerError> {
        let start = self.index;
        let string = self.consume_ident_sequence();

        if string.eq_ignore_ascii_case("url") && self.peek_literal(b"(") {
            self.advance_by(1);
            loop {
                let old_pos = self.index;
                if !self.consume_codepoint().is_some_and(is_whitespace)
                    || !self.peek_codepoint().is_some_and(is_whitespace)
                {
                    self.index = old_pos;
                    break;
                }
            }

            let is_fun = self.lookahead(|lk| match lk.consume_codepoint() {
                Some('"' | '\'') => true,
                Some(c) if c.is_whitespace() => {
                    lk.peek_codepoint().is_some_and(|c| matches!(c, '"' | '\''))
                }
                _ => false,
            });

            if is_fun {
                Ok(TokenKind::Function)
            } else {
                let Some(Range {
                    start: value_start,
                    end: value_end,
                }) = self.consume_unqouted_url()
                else {
                    return Ok(TokenKind::BadUrl);
                };

                Ok(TokenKind::Url {
                    value_offset: u16::try_from(value_start - start)
                        .map_err(|_| TokenizerError::UrlFunctionInnerWhitespaceTooLong)?,
                    trailing_len: u16::try_from(self.index - value_end)
                        .map_err(|_| TokenizerError::UrlFunctionInnerWhitespaceTooLong)?,
                })
            }
        } else if self.peek_literal(b"(") {
            self.index += 1;
            Ok(TokenKind::Function)
        } else {
            Ok(TokenKind::Ident)
        }
    }

    // https://www.w3.org/TR/css-syntax-3/#check-if-three-code-points-would-start-an-ident-sequence
    fn peek_would_start_an_ident_sequence(&mut self) -> bool {
        self.lookahead(|lk| match lk.consume_codepoint() {
            Some('-') => {
                let Some(second) = lk.consume_codepoint() else {
                    return false;
                };
                is_ident_start(second) || second == '-' || lk.peek_is_valid_escape(second)
            }
            Some(c) if is_ident_start(c) => true,
            Some('\\') => lk.peek_codepoint().is_some_and(|c| !matches!(c, '\n')),
            _ => false,
        })
    }

    // https://www.w3.org/TR/css-syntax-3/#starts-with-a-number
    fn peek_would_start_a_number(&mut self, current: char) -> bool {
        self.lookahead(|lk| match current {
            '+' | '-' => match lk.peek_codepoint() {
                Some('0'..='9') => true,
                Some('.') => lk.peek_codepoint().is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            },
            '.' => lk.peek_codepoint().is_some_and(|c| c.is_ascii_digit()),
            '0'..='9' => true,
            _ => false,
        })
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-token
    pub fn consume_token(&mut self) -> Result<Option<Token>, TokenizerError> {
        self.consume_comments();

        let start = self.index as u32;
        macro_rules! return_token {
            (with $kind: expr) => {
                return Ok(Some(Token {
                    kind: $kind,
                    span: Span { start, end: self.index as u32 }
                }))
            };
            ($($kind: tt)*) => {
                return_token!(with TokenKind::$($kind)*)
            };
        }

        match self.consume_codepoint() {
            Some(c) if is_whitespace(c) => {
                while self.peek_codepoint().is_some_and(is_whitespace) {
                    self.advance_by(1);
                }
                return_token!(Whitespace);
            }
            Some(c @ ('"' | '\'')) => {
                return_token!(with self.consume_string(c));
            }
            Some('#') => {
                if self
                    .peek_codepoint()
                    .is_some_and(|next| is_ident(next) || self.peek_is_valid_escape(next))
                {
                    let mut hash_type = HashTypeFlag::Unrestricted;
                    if self.peek_would_start_an_ident_sequence() {
                        hash_type = HashTypeFlag::Id;
                    }

                    self.consume_ident_sequence();
                    return_token!(Hash {
                        type_flag: hash_type,
                    });
                } else {
                    return_token!(Punct('#'));
                }
            }
            Some('(') => return_token!(LParen),
            Some(')') => return_token!(RParen),
            Some('+') => {
                if self.peek_would_start_a_number('+') {
                    self.reconsume('+');
                    return_token!(with self.consume_numeric_token());
                } else {
                    return_token!(Punct('+'))
                }
            }
            Some('-') => {
                if self.peek_would_start_a_number('+') {
                    self.reconsume('+');
                    return_token!(with self.consume_numeric_token());
                } else if self.peek_literal(b"->") {
                    self.index += 2;
                    return_token!(Cdc);
                } else {
                    let old = self.index;
                    self.reconsume('-');
                    if self.peek_would_start_an_ident_sequence() {
                        return_token!(with self.consume_ident_like()?);
                    } else {
                        self.index = old;
                        return_token!(Punct('-'))
                    }
                }
            }
            Some('.') => {
                if self.peek_would_start_a_number('.') {
                    self.reconsume('.');
                    return_token!(with self.consume_numeric_token());
                } else {
                    return_token!(Punct('.'));
                }
            }
            Some('<') => {
                if self.peek_literal(b"!--") {
                    self.advance_by(3);
                    return_token!(Cdo);
                } else {
                    return_token!(Punct('<'));
                }
            }
            Some('@') => {
                if self.peek_would_start_an_ident_sequence() {
                    self.consume_ident_sequence();
                    return_token!(AtKeyword);
                } else {
                    return_token!(Punct('@'));
                }
            }
            Some('[') => return_token!(LBracket),
            Some(']') => return_token!(RBracket),
            Some('{') => return_token!(LBrace),
            Some('}') => return_token!(RBrace),
            Some('\\') => {
                if self.peek_is_valid_escape('\\') {
                    self.reconsume('\\');
                    return_token!(with self.consume_ident_like()?);
                } else {
                    return_token!(Punct('\\'));
                }
            }
            Some('0'..='9') => {
                self.index -= 1;
                return_token!(with self.consume_numeric_token());
            }
            Some(c) if is_ident_start(c) => {
                self.reconsume(c);
                return_token!(with self.consume_ident_like()?);
            }
            None => Ok(None),
            Some(c) => return_token!(Punct(c)),
        }
    }

    // https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point
    fn skip_escaped_codepoint(&mut self) {
        let bytes = self.peek_bytes(6);
        let hex_len = bytes
            .iter()
            .position(|&c| !c.is_ascii_hexdigit())
            .unwrap_or(bytes.len());

        if hex_len == 0 {
            self.consume_codepoint();
            return;
        }

        self.advance_by(hex_len);
        if self.peek_codepoint().is_some_and(is_whitespace) {
            self.advance_by(1);
        }
    }
}

#[derive(Debug, Error)]
pub enum TokenizerError {
    #[error("`url(` token leading or trailing inner whitespace exceeded 2^16-1 bytes")]
    UrlFunctionInnerWhitespaceTooLong,
}

pub fn tokenize(source: &str) -> impl Iterator<Item = Result<Token, TokenizerError>> + '_ {
    let mut tokenizer = Tokenizer::new(source);
    std::iter::from_fn(move || tokenizer.consume_token().transpose())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    LParen,
    LBracket,
    LBrace,
    Function,
    RParen,
    RBracket,
    RBrace,

    Punct(char),
    Cdc,
    Cdo,
    Whitespace,
    Ident,
    AtKeyword,
    Hash {
        type_flag: HashTypeFlag,
    },
    String,
    BadString,
    Url {
        value_offset: u16,
        trailing_len: u16,
    },
    BadUrl,
    Number {
        integer: bool,
    },
    Percentage {
        integer: bool,
    },
    Dimension {
        integer: bool,
        unit_offset: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct Escaped<'a>(&'a str);

impl PartialEq for Escaped<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.unescape_iter().eq(other.unescape_iter())
    }
}

impl Eq for Escaped<'_> {}

impl PartialEq<&str> for Escaped<'_> {
    fn eq(&self, &other: &&str) -> bool {
        self.unescape_iter().eq(other.chars())
    }
}

impl From<Escaped<'_>> for Box<str> {
    fn from(value: Escaped<'_>) -> Self {
        value.unescape_iter().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashTypeFlag {
    Id,
    Unrestricted,
}

impl<'a> Escaped<'a> {
    pub fn new(escaped: &'a str) -> Self {
        Self(escaped)
    }

    pub fn unescape_iter(self) -> impl Iterator<Item = char> + use<'a> {
        let mut current = self.0.chars();
        std::iter::from_fn(move || loop {
            // Matches `Tokenizer::peek_codepoint` transformations
            fn take_as_codepoint(current: &mut std::str::Chars<'_>, next: char) -> char {
                match next {
                    '\0' => char::REPLACEMENT_CHARACTER,
                    '\x0C' => '\n',
                    '\r' => {
                        if current.as_str().starts_with("\n") {
                            current.next();
                        }
                        '\n'
                    }
                    c => c,
                }
            }

            let next = current.next()?;
            if next != '\\' {
                return Some(take_as_codepoint(&mut current, next));
            }

            let peek = current.as_str();
            let max_hex = peek.len().min(6);
            let hex_end = peek.as_bytes()[..max_hex]
                .iter()
                .position(|&b| !b.is_ascii_hexdigit())
                .unwrap_or(max_hex);

            if hex_end == 0 {
                return Some(match current.next() {
                    Some(next) => {
                        let codepoint = take_as_codepoint(&mut current, next);
                        if codepoint == '\n' {
                            // This escape is a multiline string continuation.
                            continue;
                        } else {
                            codepoint
                        }
                    }
                    None => char::REPLACEMENT_CHARACTER,
                });
            }

            let n_skip = 'skip: {
                let mut it = peek.bytes().skip(hex_end);
                let Some(next) = it.next() else {
                    break 'skip hex_end;
                };

                if is_byte_whitespace(next) || next == b'\x0C' {
                    hex_end + 1
                } else if next == b'\r' {
                    hex_end + 1 + (it.next() == Some(b'\n')) as usize
                } else {
                    hex_end
                }
            };
            current = peek[n_skip..].chars();

            let value = u64::from_str_radix(&peek[..hex_end], 16).unwrap();
            let codepoint = value
                .try_into()
                .ok()
                .and_then(char::from_u32)
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            return Some(codepoint);
        })
    }

    pub fn eq_ignore_ascii_case(self, string: &str) -> bool {
        self.unescape_iter()
            .map(|x| x.to_ascii_lowercase())
            .eq(string.chars().map(|x| x.to_ascii_lowercase()))
    }

    pub fn starts_with(self, string: &str) -> bool {
        let mut u = self.unescape_iter();

        for c in string.chars() {
            if u.next() != Some(c) {
                return false;
            }
        }

        true
    }
}

impl std::fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chr in self.unescape_iter() {
            f.write_char(chr)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::csssyn::tokenizer::{Escaped, TokenKind, Tokenizer};

    #[track_caller]
    fn assert_tokens(text: &str, tokens: &[TokenKind]) {
        let mut stream = Tokenizer::new(text);

        let mut i = 0;
        while let Some(token) = stream.consume_token().unwrap() {
            assert_eq!(token.kind, tokens[i]);
            i += 1;
        }

        if i != tokens.len() {
            panic!(
                "Premature end of token stream. Expected {len} elements but got only {i}",
                len = tokens.len()
            );
        }
    }

    #[test]
    fn unquoted_url() {
        let content = Escaped(
            r"data:image/png\3B base64\2ciVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==",
        );
        assert_eq!(content, "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==");

        assert_tokens(
            r"url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==)",
            &[TokenKind::Url {
                value_offset: 4,
                trailing_len: 1,
            }],
        );

        assert_tokens(
            r"u\72l(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==)",
            &[TokenKind::Url {
                value_offset: 6,
                trailing_len: 1,
            }],
        );

        assert_tokens(
            " u\\72l(   data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==\t\t)  ",
            &[TokenKind::Whitespace, TokenKind::Url { value_offset: 9, trailing_len: 3 }, TokenKind::Whitespace]
        );
    }

    #[test]
    fn null() {
        assert_tokens("\0llo", &[TokenKind::Ident]);
        assert_tokens("\0", &[TokenKind::Ident]);
        assert_tokens(r"\0", &[TokenKind::Ident]);
    }

    #[test]
    fn unescape() {
        assert_eq!(
            Escaped::new("\r\n\n\r\0abc\x0C\\72\\xend")
                .unescape_iter()
                .collect::<String>(),
            format!("\n\n\n{}abc\nrxend", char::REPLACEMENT_CHARACTER)
        );

        assert_eq!(
            Escaped::new("\\72\x0Cabc \\72\r\nabc \\72 abc \\72xabc")
                .unescape_iter()
                .collect::<String>(),
            "rabc rabc rabc rxabc"
        );

        assert_eq!(
            Escaped::new("\\\r\nabc \\\nabc \\\x0Cabc")
                .unescape_iter()
                .collect::<String>(),
            "abc abc abc"
        );
    }
}

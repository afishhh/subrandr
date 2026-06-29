//! https://www.w3.org/TR/css-syntax-3/#tokenization

use std::{fmt::Write, ops::Range};

#[derive(Debug, Clone)]
struct StreamPosition {
    index: usize,
    last_was_cr: bool,
}

// TODO: had_parse_error boolean?
pub(super) struct Tokenizer<'a> {
    source: &'a str,
    pos: StreamPosition,
}

pub fn is_whitespace(codepoint: char) -> bool {
    matches!(codepoint, '\n' | '\t' | ' ')
}

pub fn is_whitespace2(codepoint: u8) -> bool {
    matches!(codepoint, b'\n' | b'\t' | b' ')
}

fn is_non_printable(codepoint: char) -> bool {
    matches!(codepoint, '\0'..='\x08' | '\x0b' | '\x0e'..='\x1f' | '\x7f')
}

fn is_ident_start(codepoint: char) -> bool {
    matches!(codepoint, 'a'..='z' | 'A'..='Z' | '_') || !codepoint.is_ascii()
}

fn is_ident(codepoint: char) -> bool {
    is_ident_start(codepoint) || matches!(codepoint, '0'..='9' | '-')
}

fn is_valid_escape(a: char, b: char) -> bool {
    a == '\\' && b != '\n'
}

impl<'a> Tokenizer<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self {
            source: text,
            pos: StreamPosition {
                index: 0,
                last_was_cr: false,
            },
        }
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn position(&self) -> usize {
        self.pos.index
    }

    fn reconsume(&mut self, codepoint: char) {
        self.pos.index -= codepoint.len_utf8();
        if codepoint == '\n' && self.source.as_bytes()[self.pos.index] == b'\r' {
            self.pos.index -= 1;
        }
    }

    fn advance_by(&mut self, count_bytes: usize) {
        self.pos.index += count_bytes;
    }

    fn peek_bytes(&self, count: usize) -> &[u8] {
        &self.source.as_bytes()[self.pos.index..(self.pos.index + count).min(self.source.len())]
    }

    fn consume_comments(&mut self) {
        loop {
            if self.source.as_bytes()[self.pos.index..].starts_with(b"/*") {
                let end = self.source[self.pos.index..]
                    .find("*/")
                    .map_or(self.source.len(), |v| v + 2 + self.pos.index);
                self.pos.index = end;
            } else {
                break;
            }
        }
    }

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
                    Some(_) => {
                        self.skip_escaped_codepoint();
                    }
                },
                Some(_) => {}
            }
        }

        TokenKind::String
    }

    fn consume_ident_sequence(&mut self) -> Escaped<'a> {
        let start = self.pos.index;

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

        Escaped(&self.source[start..self.pos.index])
    }

    fn consume_number(&mut self) -> bool {
        let mut is_integer = true;

        if matches!(self.peek_codepoint(), Some('+' | '-')) {
            self.advance_by(1);
        }

        while matches!(self.peek_codepoint(), Some('0'..='9')) {
            self.advance_by(1);
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

    fn consume_numeric_token(&mut self) -> TokenKind {
        let start = self.pos.index;
        let integer = self.consume_number();

        if self.peek_would_start_an_ident_sequence() {
            let unit_offset = (self.pos.index - start) as u32;
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

    fn consume_remants_of_bad_url(&mut self) {
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

    fn consume_unqouted_url(&mut self) -> Option<Range<usize>> {
        while self.peek_codepoint().is_some_and(is_whitespace) {
            self.advance_by(1);
        }

        let start = self.pos.index;
        let end;
        loop {
            match self.consume_codepoint() {
                Some(')') => {
                    end = self.pos.index - 1;
                    break;
                }
                None => {
                    end = self.pos.index;
                    break;
                }
                Some(c) if is_whitespace(c) => {
                    end = self.pos.index - 1;

                    while self.peek_codepoint().is_some_and(is_whitespace) {
                        self.advance_by(1);
                    }

                    if self.peek_codepoint().is_none_or(|c| c == ')') {
                        self.advance_by(1);
                        break;
                    } else {
                        self.consume_remants_of_bad_url();
                        return None;
                    }
                }
                Some('"' | '\'' | '(') => {
                    self.consume_remants_of_bad_url();
                    return None;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_remants_of_bad_url();
                    return None;
                }
                Some('\\') => {
                    if self.peek_is_valid_escape('\\') {
                        self.skip_escaped_codepoint();
                    } else {
                        self.consume_remants_of_bad_url();
                        return None;
                    }
                }
                Some(_) => {}
            }
        }

        Some(start..end)
    }

    fn consume_ident_like(&mut self) -> TokenKind {
        let start = self.pos.index;
        let string = self.consume_ident_sequence();

        if string.eq_ignore_ascii_case("url") && self.peek_const("(") {
            self.advance_by(1);
            loop {
                let old = self.pos.clone();
                if !self.consume_codepoint().is_some_and(is_whitespace)
                    || !self.peek_codepoint().is_some_and(is_whitespace)
                {
                    self.pos = old;
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
                TokenKind::Function
            } else {
                let Some(Range {
                    start: value_start,
                    end: value_end,
                }) = self.consume_unqouted_url()
                else {
                    return TokenKind::BadUrl;
                };

                TokenKind::Url {
                    // TODO: checked cast
                    value_offset: (value_start - start) as u16,
                    trailing_len: (self.pos.index - value_end) as u16,
                }
            }
        } else if self.peek_const("(") {
            self.pos.index += 1;
            TokenKind::Function
        } else {
            TokenKind::Ident
        }
    }

    fn peek_codepoint(&mut self) -> Option<char> {
        loop {
            match self.source[self.pos.index..].chars().next()? {
                '\0' => {
                    self.pos.last_was_cr = false;
                    return Some(char::REPLACEMENT_CHARACTER);
                }
                '\x0C' => {
                    self.pos.last_was_cr = false;
                    return Some('\n');
                }
                '\r' => {
                    self.pos.last_was_cr = true;
                    return Some('\n');
                }
                '\n' => {
                    if self.pos.last_was_cr {
                        self.pos.last_was_cr = false;
                        self.pos.index += 1;
                        continue;
                    }
                    return Some('\n');
                }
                c => return Some(c),
            };
        }
    }

    fn consume_codepoint(&mut self) -> Option<char> {
        if let Some(chr) = self.peek_codepoint() {
            self.pos.index += chr.len_utf8();
            Some(chr)
        } else {
            None
        }
    }

    fn lookahead<T>(&mut self, callback: impl FnOnce(&mut Self) -> T) -> T {
        let old = self.pos.clone();
        let result = callback(self);
        self.pos = old;
        result
    }

    fn peek_is_valid_escape(&mut self, current: char) -> bool {
        self.peek_codepoint()
            .is_some_and(|next| is_valid_escape(current, next))
    }

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

    fn peek_const(&mut self, value: &str) -> bool {
        self.source.as_bytes()[self.pos.index..].starts_with(value.as_bytes())
    }

    pub fn consume_token(&mut self) -> Option<Token> {
        self.consume_comments();

        let start = self.pos.index;
        macro_rules! return_token {
            (with $kind: expr) => {
                return Some(Token {
                    kind: $kind,
                    len: (self.pos.index - start) as u32,
                })
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
            Some(',') => return_token!(Punct(',')),
            Some('-') => {
                if self.peek_would_start_a_number('+') {
                    self.reconsume('+');
                    return_token!(with self.consume_numeric_token());
                } else if self.peek_const("->") {
                    self.pos.index += 2;
                    return_token!(Cdc);
                } else {
                    let old = self.pos.clone();
                    self.reconsume('-');
                    if self.peek_would_start_an_ident_sequence() {
                        return_token!(with self.consume_ident_like());
                    } else {
                        self.pos = old;
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
            Some(':') => return_token!(Punct(':')),
            Some(';') => return_token!(Punct(';')),
            Some('<') => {
                if self.peek_const("!--") {
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
                    return_token!(with self.consume_ident_like());
                } else {
                    return_token!(Punct('\\'));
                }
            }
            Some('0'..='9') => {
                self.pos.index -= 1;
                return_token!(with self.consume_numeric_token());
            }
            Some(c) if is_ident_start(c) => {
                self.reconsume(c);
                return_token!(with self.consume_ident_like());
            }
            None => None,
            Some(c) => return_token!(Punct(c)),
        }
    }

    fn skip_escaped_codepoint(&mut self) {
        let hex_len = self
            .peek_bytes(6)
            .iter()
            .position(|&c| !c.is_ascii_hexdigit())
            .map_or(6, |v| v);

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

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        self.consume_token()
    }
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
    pub len: u32,
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
            let next = current.next()?;
            if next != '\\' {
                return Some(next);
            }

            let peek = current.as_str();
            let max_hex = peek.len().min(6);
            let hex_end = peek.as_bytes()[..max_hex]
                .iter()
                .position(|&b| !b.is_ascii_hexdigit())
                .unwrap_or(max_hex);

            if hex_end == 0 {
                let next = current.next();
                match next {
                    Some('\n') => continue,
                    Some('\r') if current.as_str().starts_with("\n") => {
                        continue;
                    }
                    Some(c) => return Some(c),
                    None => return Some(char::REPLACEMENT_CHARACTER),
                }
            }

            let n_skip = 'skip: {
                let mut it = peek.bytes().skip(hex_end);
                let Some(next) = it.next() else {
                    break 'skip hex_end;
                };

                if is_whitespace2(next) {
                    hex_end + 1
                } else if next == b'\r' {
                    hex_end + 1 + (it.next() == Some(b'\n')) as usize
                } else {
                    hex_end
                }
            };
            current = peek[n_skip..].chars();

            let value = u64::from_str_radix(&peek[..hex_end], 16).unwrap();
            return Some(
                value
                    .try_into()
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or(char::REPLACEMENT_CHARACTER),
            );
        })
    }

    pub fn eq_ignore_ascii_case(self, string: &str) -> bool {
        self.unescape_iter().eq(string.chars())
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
        while let Some(token) = stream.consume_token() {
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
}

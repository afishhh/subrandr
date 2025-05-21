//! https://www.w3.org/TR/css-syntax-3/#tokenization

#[derive(Debug, Clone)]
struct StreamPosition {
    index: usize,
    last_was_cr: bool,
}

// TODO: had_parse_error boolean?
pub struct InputStream<'a> {
    source: &'a str,
    pos: StreamPosition,
    temporary_buffer: String,
}

pub fn is_whitespace(codepoint: char) -> bool {
    matches!(codepoint, '\n' | '\t' | ' ')
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

impl<'a> InputStream<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self {
            source: text,
            pos: StreamPosition {
                index: 0,
                last_was_cr: false,
            },
            temporary_buffer: String::new(),
        }
    }

    pub fn fork(&self) -> Self {
        Self {
            source: self.source,
            pos: self.pos.clone(),
            temporary_buffer: String::new(),
        }
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
        self.temporary_buffer.clear();

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
                        let e = self.consume_escaped_codepoint();
                        self.temporary_buffer.push(e);
                    }
                },
                Some(c) => self.temporary_buffer.push(c),
            }
        }

        return TokenKind::String(self.temporary_buffer.as_str().into());
    }

    fn consume_ident_sequence(&mut self) -> Box<str> {
        self.temporary_buffer.clear();

        loop {
            match self.consume_codepoint() {
                Some(c) if is_ident(c) => {
                    self.temporary_buffer.push(c);
                }
                Some(c) if self.peek_is_valid_escape(c) => {
                    let e = self.consume_escaped_codepoint();
                    self.temporary_buffer.push(e);
                }
                Some(c) => {
                    self.reconsume(c);
                    break;
                }
                None => break,
            }
        }

        self.temporary_buffer.as_str().into()
    }

    fn consume_number(&mut self) -> (f64, bool) {
        let start = self.pos.index;
        let mut is_integer = true;

        if matches!(self.peek_codepoint(), Some('+' | '-')) {
            self.advance_by(1);
        }

        while matches!(self.peek_codepoint(), Some('0'..='9')) {
            self.advance_by(1);
        }

        let peek = self.peek_bytes(3);
        let mut i = 1;
        if matches!(peek.get(0), Some(b'E' | b'e')) && {
            i += matches!(peek.get(1), Some(b'+' | b'-')) as usize;
            matches!(peek.get(i), Some(b'0'..=b'9'))
        } {
            self.advance_by(i + 1);
            is_integer = false;

            while matches!(self.peek_codepoint(), Some('0'..='9')) {
                self.advance_by(1);
            }
        }

        (
            self.source[start..self.pos.index].parse().unwrap(),
            is_integer,
        )
    }

    fn consume_numeric_token(&mut self) -> TokenKind {
        let number = self.consume_number();

        if self.peek_would_start_an_ident_sequence() {
            return TokenKind::Dimension(DimensionToken {
                value: number.0,
                integer: number.1,
                unit: self.consume_ident_sequence(),
            });
        } else if self.peek_codepoint().is_some_and(|c| c == '%') {
            self.advance_by(1);
            return TokenKind::Percentage(PercentageToken { value: number.0 });
        } else {
            return TokenKind::Number(NumberToken {
                value: number.0,
                integer: number.1,
            });
        }
    }

    fn consume_remants_of_bad_url(&mut self) {
        match self.consume_codepoint() {
            Some(')') | None => return,
            Some('\\') => {
                if self.peek_is_valid_escape('\\') {
                    self.consume_escaped_codepoint();
                }
            }
            Some(_) => (),
        }
    }

    fn consume_unqouted_url(&mut self) -> TokenKind {
        self.temporary_buffer.clear();

        while self.peek_codepoint().is_some_and(is_whitespace) {
            self.advance_by(1);
        }

        loop {
            match self.consume_codepoint() {
                Some(')') => break,
                None => break,
                Some(c) if is_whitespace(c) => {
                    while self.peek_codepoint().is_some_and(is_whitespace) {
                        self.advance_by(1);
                    }

                    if self.peek_codepoint().is_none_or(|c| c == ')') {
                        self.advance_by(1);
                        break;
                    } else {
                        self.consume_remants_of_bad_url();
                        return TokenKind::BadUrl;
                    }
                }
                Some('"' | '\'' | '(') => {
                    self.consume_remants_of_bad_url();
                    return TokenKind::BadUrl;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_remants_of_bad_url();
                    return TokenKind::BadUrl;
                }
                Some('\\') => {
                    if self.peek_is_valid_escape('\\') {
                        let e = self.consume_escaped_codepoint();
                        self.temporary_buffer.push(e);
                    } else {
                        self.consume_remants_of_bad_url();
                        return TokenKind::BadUrl;
                    }
                }
                Some(c) => self.temporary_buffer.push(c),
            }
        }

        TokenKind::Url(self.temporary_buffer.as_str().into())
    }

    fn consume_ident_like(&mut self) -> TokenKind {
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
                TokenKind::Function(string)
            } else {
                self.consume_unqouted_url()
            }
        } else if self.peek_const("(") {
            self.pos.index += 1;
            return TokenKind::Function(string);
        } else {
            return TokenKind::Ident(string);
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

    pub fn consume_token(&mut self) -> Option<Token<'a>> {
        self.consume_comments();

        let start = self.pos.index;
        macro_rules! return_token {
            (with $kind: expr) => {
                return Some(Token {
                    kind: $kind,
                    representation: &self.source[start..self.pos.index],
                })
            };
            ($kind: ident $(, $value: expr)?) => {
                return_token!(with TokenKind::$kind $(($value))?)
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

                    return_token!(
                        Hash,
                        HashToken {
                            value: self.consume_ident_sequence(),
                            type_flag: hash_type,
                        }
                    );
                } else {
                    return_token!(Delim, '#');
                }
            }
            Some('(') => return_token!(LParen),
            Some(')') => return_token!(RParen),
            Some('+') => {
                if self.peek_would_start_a_number('+') {
                    self.reconsume('+');
                    return_token!(with self.consume_numeric_token());
                } else {
                    return_token!(Delim, '+')
                }
            }
            Some(',') => return_token!(Comma),
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
                        return_token!(Delim, '-')
                    }
                }
            }
            Some('.') => {
                if self.peek_would_start_a_number('.') {
                    self.reconsume('.');
                    return_token!(with self.consume_numeric_token());
                } else {
                    return_token!(Delim, '.');
                }
            }
            Some(':') => return_token!(Colon),
            Some(';') => return_token!(Semicolon),
            Some('<') => {
                if self.peek_const("!--") {
                    self.advance_by(3);
                    return_token!(Cdo);
                } else {
                    return_token!(Delim, '<');
                }
            }
            Some('@') => {
                if self.peek_would_start_an_ident_sequence() {
                    let string = self.consume_ident_sequence();
                    return_token!(AtKeyword, string);
                } else {
                    return_token!(Delim, '@');
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
                    return_token!(Delim, '\\');
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
            Some(c) => return_token!(Delim, c),
        }
    }

    fn consume_escaped_codepoint(&mut self) -> char {
        let start = self.pos.index;
        match self.consume_codepoint() {
            Some('0'..='9' | 'A'..='F' | 'a'..='f') => {
                let end = self.source[self.pos.index + 1..self.pos.index + 6]
                    .find(|c: char| !c.is_ascii_hexdigit())
                    .map_or(self.pos.index + 6, |v| self.pos.index + 1 + v);
                self.pos.index = end;
                if self.peek_codepoint().is_some_and(is_whitespace) {
                    self.advance_by(1);
                }
                let value = u64::from_str_radix(&self.source[start..end], 16).unwrap();

                value
                    .try_into()
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or(char::REPLACEMENT_CHARACTER)
            }
            None => char::REPLACEMENT_CHARACTER,
            Some(c) => c,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    LParen,
    RParen,
    Comma,
    Cdc,
    Cdo,
    Colon,
    Semicolon,
    Whitespace,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Ident(Box<str>),
    Function(Box<str>),
    AtKeyword(Box<str>),
    Hash(HashToken),
    String(Box<str>),
    BadString,
    Url(Box<str>),
    BadUrl,
    Delim(char),
    Number(NumberToken),
    Percentage(PercentageToken),
    Dimension(DimensionToken),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub representation: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashToken {
    pub value: Box<str>,
    pub type_flag: HashTypeFlag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashTypeFlag {
    Id,
    Unrestricted,
}

#[derive(Debug, Clone, Copy)]
pub struct NumberToken {
    pub value: f64,
    pub integer: bool,
}

impl Eq for NumberToken {}

impl PartialEq for NumberToken {
    fn eq(&self, other: &Self) -> bool {
        self.value.total_cmp(&other.value).is_eq() && self.integer == other.integer
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PercentageToken {
    pub value: f64,
}

impl Eq for PercentageToken {}

impl PartialEq for PercentageToken {
    fn eq(&self, other: &Self) -> bool {
        self.value.total_cmp(&other.value).is_eq()
    }
}

#[derive(Debug, Clone)]
pub struct DimensionToken {
    pub value: f64,
    pub integer: bool,
    pub unit: Box<str>,
}

impl Eq for DimensionToken {}

impl PartialEq for DimensionToken {
    fn eq(&self, other: &Self) -> bool {
        self.value.total_cmp(&other.value).is_eq()
            && self.integer == other.integer
            && self.unit == other.unit
    }
}

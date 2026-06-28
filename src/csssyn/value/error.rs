use super::{Span, Spanned};

#[derive(Debug)]
pub struct ParseError {
    messages: Vec<ErrorMessage>,
}

#[derive(Debug)]
struct ErrorMessage {
    span: Span,
    message: String,
}

impl ParseError {
    pub fn new(span: impl Spanned, message: impl std::fmt::Display) -> Self {
        Self {
            messages: vec![ErrorMessage {
                span: span.span(),
                message: message.to_string(),
            }],
        }
    }

    pub fn append(&mut self, mut other: ParseError) {
        self.messages.append(&mut other.messages);
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = &self.messages[0];
        write!(f, "{}..{}: {}", msg.span.start, msg.span.end, msg.message)?;
        if let Some(remaining_messages) = self.messages.len().checked_sub(1) {
            write!(f, " (and {} others)", remaining_messages);
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

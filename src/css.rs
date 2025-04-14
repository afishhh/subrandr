fn is_whitespace(codepoint: char) -> bool {
    matches!(codepoint, '\n' | '\t' | ' ')
}

mod tokenizer;
pub use tokenizer::*;
mod stylesheet;
pub use stylesheet::*;

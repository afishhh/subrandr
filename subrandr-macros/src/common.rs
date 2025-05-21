use syn::parse::ParseStream;

use crate::parse::*;

pub fn parse_yes_no(stream: ParseStream) -> syn::Result<bool> {
    let lookahead1 = stream.lookahead1();
    let lk = lookahead1;
    if lk.peek(kw::yes) {
        stream.parse::<kw::yes>()?;
        Ok(true)
    } else if lk.peek(kw::no) {
        stream.parse::<kw::no>()?;
        Ok(false)
    } else {
        Err(lk.error())
    }
}

pub fn advance_past_punct(stream: ParseStream, chr: char) {
    loop {
        match stream.parse::<TokenTree2>() {
            Ok(TokenTree2::Punct(punct)) if punct.as_char() == chr => {
                break;
            }
            Err(_) => break,
            _ => (),
        }
    }
}

pub mod kw {
    syn::custom_keyword!(yes);
    syn::custom_keyword!(no);
}

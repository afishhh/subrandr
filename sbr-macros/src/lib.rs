mod common;
mod parse;
mod style;

#[proc_macro]
pub fn implement_style_module(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    style::implement_style_module_impl(ts)
}

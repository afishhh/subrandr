mod common;
mod parse;
mod style;
mod test;

#[proc_macro]
pub fn implement_style_module(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    style::implement_style_module_impl(ts)
}

#[proc_macro]
pub fn test_define_style(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    test::test_define_style(ts)
}

#[proc_macro]
pub fn test_apply_style(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    test::test_apply_style(ts)
}

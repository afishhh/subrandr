mod common;
mod css;
mod parse;
mod style;

#[proc_macro]
pub fn implement_style_module(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    style::implement_style_module_impl(ts)
}

#[proc_macro]
pub fn make_css_property_parser_list(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    css::make_css_property_parser_list_impl(ts)
}

#[proc_macro]
pub fn make_css_value_parser(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    css::make_css_value_parser_impl(ts)
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;
use syn::parse_quote;
use syn::ItemFn;

fn modify_function(mut function: ItemFn) -> ItemFn {
    function
        .block
        .stmts
        .insert(0, parse_quote! { mononoke::override_just_knobs(); });
    function
}

#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let _ = parse_macro_input!(args as syn::parse::Nothing);
    let function = parse_macro_input!(input as ItemFn);

    let modified_function = modify_function(function);

    quote! {
        #[test]
        #modified_function
    }
    .into()
}

#[proc_macro_attribute]
pub fn fbinit_test(args: TokenStream, input: TokenStream) -> TokenStream {
    let _ = parse_macro_input!(args as syn::parse::Nothing);
    let function = parse_macro_input!(input as ItemFn);

    let modified_function = modify_function(function);

    quote! {
        #[fbinit::test]
        #modified_function
    }
    .into()
}

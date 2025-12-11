/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use proc_macro::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::Error;
use syn::FnArg;
use syn::ItemFn;
use syn::Pat;
use syn::Type;
use syn::parse_macro_input;
use syn::parse_quote;
use syn::punctuated::Punctuated;
use syn::token::Comma;

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

    let modified_function = modify_function(function.clone());

    // If the function is async, delegate to `tokio::test`
    let test_attribute = if function.sig.asyncness.is_some() {
        quote! { #[tokio::test] }
    } else {
        quote! { #[test] }
    };

    quote! {
        #test_attribute
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

fn parse_quickcheck_args(
    fn_item: &ItemFn,
) -> Result<(Punctuated<Pat, Comma>, Punctuated<Type, Comma>), TokenStream> {
    let mut ids = Punctuated::new();
    let mut tys = Punctuated::new();

    for pt in fn_item.sig.inputs.iter() {
        match pt {
            FnArg::Receiver(_) => {
                return Err(
                    Error::new_spanned(fn_item, "test fn cannot take a receiver")
                        .to_compile_error()
                        .into(),
                );
            }
            FnArg::Typed(pt) => {
                ids.push(*pt.pat.clone());
                tys.push(*pt.ty.clone());
            }
        }
    }

    Ok((ids, tys))
}

#[proc_macro_attribute]
pub fn quickcheck_test(args: TokenStream, input: TokenStream) -> TokenStream {
    let _ = parse_macro_input!(args as syn::parse::Nothing);
    let fn_item = parse_macro_input!(input as ItemFn);

    if fn_item.sig.asyncness.is_none() {
        return Error::new_spanned(&fn_item, "test fn must be async")
            .to_compile_error()
            .into();
    }

    let call_by = format_ident!("{}", fn_item.sig.ident);

    let (ids, tys) = match parse_quickcheck_args(&fn_item) {
        Err(e) => return e,
        Ok(args) => args,
    };

    let ret = &fn_item.sig.output;

    quote! {
        #[::tokio::test]
        async fn #call_by() {
            mononoke::override_just_knobs();

            #fn_item

            let test_fn: fn(#tys) #ret = |#ids| {
                ::futures::executor::block_on(#call_by(#ids))
            };

            ::tokio::task::spawn_blocking(move || {
                ::quickcheck::quickcheck(test_fn)
            })
            .await
            .unwrap()
        }
    }
    .into()
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use proc_macro2::TokenStream;
use quote::quote;
use syn::Error;
use syn::ItemFn;
use syn::Result;
use syn::parse_quote;
use syn::punctuated::Punctuated;

use crate::args::Args;
use crate::args::DisableFatalSignals;

#[derive(Copy, Clone, PartialEq)]
pub enum Mode {
    Main,
    Test,
    NestedTest,
}

pub fn expand(mode: Mode, args: Args, mut function: ItemFn) -> Result<TokenStream> {
    if mode != Mode::NestedTest && function.sig.inputs.len() > 1 {
        return Err(Error::new_spanned(
            function.sig,
            "expected one argument of type fbinit::FacebookInit unless #[fbinit::nested_test] is used",
        ));
    }

    if mode == Mode::Main && function.sig.ident != "main" {
        return Err(Error::new_spanned(
            function.sig,
            "#[fbinit::main] must be used on the main function",
        ));
    }

    let guard = match mode {
        Mode::Main => Some(quote! {
            if module_path!().contains("::") {
                panic!("fbinit must be performed in the crate root on the main function");
            }
        }),
        _ => None,
    };

    let set_vars = args
        .vars
        .iter()
        .map(|(key, var)| {
            quote! {
                unsafe { std::env::set_var(#key, #var) };
            }
        })
        .collect::<Vec<_>>();

    let set_vars = quote! { #(#set_vars)* };

    let assignment = function.sig.inputs.first().map(|arg| quote!(let #arg =));
    match mode {
        Mode::NestedTest => {
            // remove the first input (fb: FacebookInit) from function signature
            function.sig.inputs = function.sig.inputs.into_iter().skip(1).collect();
        }
        _ => {
            function.sig.inputs = Punctuated::new();
        }
    }

    let block = function.block;

    let body = match (function.sig.asyncness.is_some(), mode) {
        (true, Mode::Test | Mode::NestedTest) => {
            let tokio_workers = match args.tokio_workers {
                Some(tokio_workers) => quote!(::std::option::Option::Some(#tokio_workers)),
                None => quote!(::std::option::Option::None),
            };
            quote! {
                fbinit_tokio::tokio_test(#tokio_workers, async #block )
            }
        }
        (true, Mode::Main) => {
            let tokio_workers = match args.tokio_workers {
                Some(tokio_workers) => quote!(::std::option::Option::Some(#tokio_workers)),
                None => quote!(::std::option::Option::None),
            };
            quote! {
                fbinit_tokio::tokio_main(#tokio_workers, async #block )
            }
        }
        (false, _) => {
            let stmts = block.stmts;
            quote! { #(#stmts)* }
        }
    };

    let perform_init = match args.disable_fatal_signals {
        DisableFatalSignals::Default => {
            // 8002 is 1 << 15 (SIGTERM) | 1 << 2 (SIGINT)
            quote! {
                fbinit::perform_init_with_disable_signals(0x8002)
            }
        }
        DisableFatalSignals::All => {
            // ffff is a mask of all 1's
            quote! {
                fbinit::perform_init_with_disable_signals(0xffff)
            }
        }
        DisableFatalSignals::SigtermOnly => {
            // 8000 is 1 << 15 (SIGTERM)
            quote! {
                fbinit::perform_init_with_disable_signals(0x8000)
            }
        }
        DisableFatalSignals::None => {
            quote! {
                fbinit::perform_init()
            }
        }
    };

    function.block = parse_quote!({
        #guard
        #set_vars
        #assignment unsafe {
            #perform_init
        };
        let destroy_guard = unsafe { fbinit::DestroyGuard::new() };
        #body
    });

    function.sig.asyncness = None;

    if mode == Mode::Test {
        function.attrs.push(parse_quote!(#[test]));
    }

    Ok(quote!(#function))
}

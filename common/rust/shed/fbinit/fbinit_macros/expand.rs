/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
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
            // 8002 is 1 << 15 (SIGTERM) | 1 << 1 (SIGINT)
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

    // Tests get NO DestroyGuard: `perform_destroy` tears down process-global
    // C++ state (folly singletons, the global IO executor), and a test binary
    // may run many test cases in one process (tpx `run_as_bundle`, or invoking
    // the test binary directly) — the first finishing test's guard would
    // destroy that state under every still-running sibling, and `perform_init`
    // is a `Once`, so nothing can ever bring it back. The per-test guard only
    // ever appeared safe because tpx's default one-process-per-case isolation
    // made "end of test" and "end of program" coincide. Tests exit right after
    // the last case, so skipping the destructor changes nothing observable;
    // the init stays reachable from a static, so leak checkers report it as
    // still-reachable, not leaked.
    let destroy_guard = match mode {
        Mode::Main => quote! {
            let destroy_guard = unsafe { fbinit::DestroyGuard::new() };
        },
        Mode::Test | Mode::NestedTest => quote! {},
    };

    function.block = parse_quote!({
        #guard
        #set_vars
        #assignment unsafe {
            #perform_init
        };
        #destroy_guard
        #body
    });

    function.sig.asyncness = None;

    if mode == Mode::Test {
        function.attrs.push(parse_quote!(#[test]));
    }

    Ok(quote!(#function))
}

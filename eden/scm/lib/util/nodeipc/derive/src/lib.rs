/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

extern crate proc_macro;
use proc_macro::Delimiter;
use proc_macro::Group;
use proc_macro::Ident;
use proc_macro::Punct;
use proc_macro::Spacing;
use proc_macro::Span;
use proc_macro::TokenStream;
use proc_macro::TokenTree;

/// Attribute version of `define_ipc!`.
#[proc_macro_attribute]
pub fn ipc(attr: TokenStream, stream: TokenStream) -> TokenStream {
    let mut new_stream = TokenStream::new();
    // Wrap in `::nodeipc::define_ipc! { ... }`
    if attr.to_string() != "test" {
        // Skip `::nodeipc::` for `#[ipc(test)]`.
        // `::nodeipc` does not resolve inside `nodeipc`.
        new_stream.extend([
            TokenTree::Punct(Punct::new(':', Spacing::Joint)),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
            TokenTree::Ident(Ident::new("nodeipc", Span::call_site())),
            TokenTree::Punct(Punct::new(':', Spacing::Joint)),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
        ]);
    }
    new_stream.extend([
        TokenTree::Ident(Ident::new("define_ipc", Span::call_site())),
        TokenTree::Punct(Punct::new('!', Spacing::Alone)),
        TokenTree::Group(Group::new(Delimiter::Brace, stream)),
    ]);
    new_stream
}

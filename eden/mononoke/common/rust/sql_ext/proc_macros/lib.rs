/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use proc_macro::TokenStream;
use quote::quote;
use syn::TypeTuple;
use syn::parse_macro_input;
use syn::parse_quote;

/// Extracts any args of type RepositoryId passed inside a list (i.e. to write
/// multiple rows in a single query).
#[proc_macro]
pub fn extract_repo_ids_from_values(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as TypeTuple);

    let mut repo_ids_at_index = vec![];

    for (i, ty) in input.elems.into_iter().enumerate() {
        let index = syn::Index::from(i);
        if ty == parse_quote!(RepositoryId) {
            repo_ids_at_index.push(quote! {
                repo_ids_from_values.extend(values.iter().map(|v| v. #index));
            });
        }
    }

    quote! {
        {
            let mut repo_ids_from_values = vec![];
            #( #repo_ids_at_index )*
            repo_ids_from_values
        }
    }
    .into()
}

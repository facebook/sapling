/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

pub(crate) use proc_macro2::TokenStream;
pub(crate) use proc_macro2::TokenTree;
pub(crate) use quote::format_ident;
pub(crate) use quote::quote;
pub(crate) use tree_pattern_match::Match;
pub(crate) use tree_pattern_match::PlaceholderExt as _;

pub(crate) use crate::token::TokenInfo;
pub(crate) use crate::token_stream_ext::AngleBracket;
pub(crate) use crate::token_stream_ext::FindReplace;
pub(crate) use crate::token_stream_ext::MatchExt;
pub(crate) use crate::token_stream_ext::PlaceholderExt as _;
pub(crate) use crate::token_stream_ext::ToItems;
pub(crate) use crate::token_stream_ext::ToTokens;

pub(crate) type Item = tree_pattern_match::Item<TokenInfo>;

pub(crate) fn parse(code: &str) -> TokenStream {
    TokenStream::from_str(code).unwrap()
}

pub(crate) fn unparse(stream: impl ToTokens) -> String {
    let tokens = stream.to_tokens();
    if cfg!(test) {
        // Break and indent { } blocks for easier reading.
        use proc_macro2::Delimiter;
        use proc_macro2::TokenTree;

        fn indent(s: &str, prefix: &str) -> String {
            s.lines()
                .filter_map(|s| {
                    if s.trim().is_empty() {
                        None
                    } else {
                        Some(format!("{prefix}{s}\n"))
                    }
                })
                .collect::<Vec<_>>()
                .concat()
        }

        std::thread_local! {
            static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        }

        DEPTH.set(DEPTH.get() + 1);

        let mut result = String::new();
        let mut next_need_space = false;
        for token in tokens {
            let need_space = next_need_space;
            next_need_space = true;
            match &token {
                TokenTree::Group(g) if matches!(g.delimiter(), Delimiter::Brace) => {
                    let inner_str = unparse(g.stream());
                    if inner_str.len() > 12 {
                        // indent it
                        let inner_str = indent(&inner_str, "    ");
                        result.push_str(" {\n");
                        result.push_str(&inner_str);
                        result.push_str("}\n");
                        continue;
                    }
                }
                TokenTree::Punct(p) => {
                    next_need_space = matches!(p.spacing(), proc_macro2::Spacing::Alone);
                    if p.as_char() == ';' {
                        result.push_str(";\n");
                        continue;
                    }
                }
                _ => {}
            }

            if need_space && !result.ends_with('\n') {
                result.push(' ');
            }
            result.push_str(&token.to_string());
        }

        // Indent at the outmost level to make it easier to read in test assert_eq!s.
        DEPTH.set(DEPTH.get() - 1);
        if DEPTH.get() == 0 && result.contains('\n') {
            result = format!("\n{}", indent(&result, "            ").trim_end());
        }

        result
    } else {
        tokens.to_string()
    }
}

pub(crate) fn pick_unique_name(body: Vec<Item>, preferred_name: &str) -> TokenStream {
    let mut name = preferred_name.to_string();
    while !body.find_all(name.as_str()).is_empty() {
        name.push('_');
    }
    parse(&name)
}

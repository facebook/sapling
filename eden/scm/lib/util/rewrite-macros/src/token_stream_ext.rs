/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use proc_macro2::Group;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use tree_pattern_match::find_all;
use tree_pattern_match::replace_all;
use tree_pattern_match::Match;
use tree_pattern_match::Replace;

use crate::prelude::Item;
use crate::prelude::TokenInfo;

pub(crate) trait FindReplace {
    fn replace_with(&self, pat: impl ToItems, replace: impl Replace<TokenInfo>) -> Self;
    fn replace(&self, pat: impl ToItems, replace: impl ToItems) -> Self
    where
        Self: Sized,
    {
        self.replace_with(pat, replace.to_items())
    }
    fn find_all(&self, pat: impl ToItems) -> Vec<Match<TokenInfo>>;
}

pub(crate) trait ToItems {
    fn to_items(&self) -> Vec<Item>;
}

pub(crate) trait ToTokens {
    fn to_tokens(self) -> TokenStream;
}

impl ToItems for TokenStream {
    fn to_items(&self) -> Vec<Item> {
        self.clone()
            .into_iter()
            .map(|tt| match tt {
                TokenTree::Group(ref v) => {
                    let sub_items = v.stream().to_items();
                    Item::Tree(TokenInfo::from(tt), sub_items)
                }
                TokenTree::Ident(v) if v.to_string().starts_with("__") => {
                    Item::Placeholder(v.to_string())
                }
                _ => {
                    let token = TokenInfo::from(tt);
                    Item::Item(token)
                }
            })
            .collect()
    }
}

impl ToItems for Vec<Item> {
    fn to_items(&self) -> Vec<Item> {
        self.clone()
    }
}

impl ToItems for &'_ Vec<Item> {
    fn to_items(&self) -> Vec<Item> {
        self.to_vec()
    }
}

impl ToItems for &'_ [Item] {
    fn to_items(&self) -> Vec<Item> {
        self.to_vec()
    }
}

impl ToItems for &'_ str {
    fn to_items(&self) -> Vec<Item> {
        TokenStream::from_str(self).unwrap().to_items()
    }
}

impl ToTokens for TokenStream {
    fn to_tokens(self) -> TokenStream {
        self
    }
}

impl ToTokens for &'_ TokenStream {
    fn to_tokens(self) -> TokenStream {
        self.clone()
    }
}

impl ToTokens for Vec<Item> {
    fn to_tokens(self) -> TokenStream {
        let items = self;
        let iter = items.into_iter().map(|item| match item {
            Item::Tree(info, sub_items) => {
                let stream = sub_items.to_tokens();
                let delimiter = match info {
                    TokenInfo::Group(v) => v,
                    _ => panic!("Item::Tree should capture TokenInfo::Group"),
                };
                let new_group = Group::new(delimiter, stream);
                TokenTree::Group(new_group)
            }
            Item::Item(info) => match info {
                TokenInfo::Atom(v) => v,
                _ => panic!("Item::Item should capture TokenInfo::Atom"),
            },
            Item::Placeholder(v) => panic!("cannot convert placeholder {} back to Token", v),
        });
        TokenStream::from_iter(iter)
    }
}

impl ToTokens for &'_ Vec<Item> {
    fn to_tokens(self) -> TokenStream {
        self.clone().to_tokens()
    }
}

impl ToTokens for &'_ [Item] {
    fn to_tokens(self) -> TokenStream {
        self.to_vec().to_tokens()
    }
}

impl FindReplace for TokenStream {
    fn replace_with(&self, pat: impl ToItems, replace: impl Replace<TokenInfo>) -> Self {
        let pat = pat.to_items();
        let items = self.to_items();
        let items = replace_all(items, &pat, replace);
        items.to_tokens()
    }

    fn find_all(&self, pat: impl ToItems) -> Vec<Match<TokenInfo>> {
        let items = self.to_items();
        let pat = pat.to_items();
        find_all(&items, &pat)
    }
}

impl FindReplace for Vec<Item> {
    fn replace_with(&self, pat: impl ToItems, replace: impl Replace<TokenInfo>) -> Self {
        let pat = pat.to_items();
        let items = self.clone();
        replace_all(items, &pat, replace)
    }

    fn find_all(&self, pat: impl ToItems) -> Vec<Match<TokenInfo>> {
        let pat = pat.to_items();
        find_all(self, &pat)
    }
}

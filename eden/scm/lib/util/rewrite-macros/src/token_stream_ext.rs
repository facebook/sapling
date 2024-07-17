/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use proc_macro::Group;
use proc_macro::TokenStream;
use proc_macro::TokenTree;
use tree_pattern_match::find_all;
use tree_pattern_match::replace_all;
use tree_pattern_match::Match;

use crate::Item;
use crate::TokenInfo;

/// Convenient methods on TokenStream.
pub(crate) trait TokenStreamExt {
    fn to_items(&self) -> Vec<Item>;
    fn from_items(items: Vec<Item>) -> Self;
    fn replace_all(&mut self, pat: Self, replace: Self) -> &mut Self;
    fn replace_all_raw(&mut self, pat: &[Item], replace: &[Item]) -> &mut Self;
    fn find_all(&self, pat: Self) -> Vec<Match<TokenInfo>>;
}

impl TokenStreamExt for TokenStream {
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

    fn from_items(items: Vec<Item>) -> Self {
        let iter = items.into_iter().map(|item| match item {
            Item::Tree(info, sub_items) => {
                let stream = Self::from_items(sub_items);
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

    fn replace_all(&mut self, pat: Self, replace: Self) -> &mut Self {
        let pat = pat.to_items();
        let replace = replace.to_items();
        self.replace_all_raw(&pat, &replace)
    }

    fn replace_all_raw(&mut self, pat: &[Item], replace: &[Item]) -> &mut Self {
        let items = self.to_items();
        let items = replace_all(items, pat, replace);
        *self = Self::from_items(items);
        self
    }

    fn find_all(&self, pat: Self) -> Vec<Match<TokenInfo>> {
        let items = self.to_items();
        let pat = pat.to_items();
        find_all(&items, &pat)
    }
}

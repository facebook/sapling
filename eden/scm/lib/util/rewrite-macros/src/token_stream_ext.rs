/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::Punct;
use proc_macro2::Spacing;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use tree_pattern_match::find_all;
use tree_pattern_match::matches_full;
use tree_pattern_match::replace_all;
use tree_pattern_match::Match;
use tree_pattern_match::Placeholder;
use tree_pattern_match::PlaceholderExt as _;
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
    fn matches_full(&self, pat: impl ToItems) -> Option<Match<TokenInfo>>;
}

pub(crate) trait AngleBracket {
    /// Change `< ... >` into group to avoid unbalanced matches.
    /// Only use this when standalone `<` (less than) or `>` are not possible.
    fn group_by_angle_bracket(self) -> Self;
}

pub(crate) trait ToItems {
    fn to_items(&self) -> Vec<Item>;
}

pub(crate) trait ToTokens {
    fn to_tokens(self) -> TokenStream;
}

pub(crate) trait MatchExt {
    fn captured_tokens(&self, name: &str) -> TokenStream;
}

pub(crate) trait PlaceholderExt {
    fn disallow_group_match(self, name: &'static str) -> Self;
}

impl ToItems for TokenStream {
    fn to_items(&self) -> Vec<Item> {
        let mut iter = self.clone().into_iter().peekable();
        let mut result = Vec::with_capacity(iter.size_hint().0);
        while let Some(tt) = iter.next() {
            let next = iter.peek();
            let item = match (&tt, next) {
                (TokenTree::Group(v), _) => {
                    let sub_items = v.stream().to_items();
                    Item::Tree(TokenInfo::from_single(tt), sub_items)
                }
                (TokenTree::Ident(v), _) if v.to_string().starts_with("__") => {
                    Item::Placeholder(Placeholder::new(v.to_string()))
                }
                (TokenTree::Punct(p1), Some(TokenTree::Punct(p2)))
                    if is_punct_pair_atom(p1, p2) =>
                {
                    let tokens = vec![tt, iter.next().unwrap()];
                    let token = TokenInfo::from_multi(tokens);
                    Item::Item(token)
                }
                _ => {
                    let token = TokenInfo::from_single(tt);
                    Item::Item(token)
                }
            };
            result.push(item);
        }
        result
    }
}

// Only allow unambigious atoms.
// ex. ">>" is ambigious since it can be part of "Result<Vec<T>>".
fn is_punct_pair_atom(p1: &Punct, p2: &Punct) -> bool {
    matches!(p1.spacing(), Spacing::Joint)
        && matches!(
            (p1.as_char(), p2.as_char()),
            (':', ':') | ('-', '>') | ('=', '>')
        )
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
        let iter = items.into_iter().flat_map(|item| match item {
            Item::Tree(info, sub_items) => {
                let stream = sub_items.to_tokens();
                match info {
                    TokenInfo::Group(delimiter) => {
                        let new_group = Group::new(delimiter, stream);
                        vec![TokenTree::Group(new_group)]
                    }
                    TokenInfo::CustomGroup(l, r) => {
                        let mut result = vec![l];
                        result.extend(stream);
                        result.push(r);
                        result
                    }
                    _ => panic!("Item::Tree should capture TokenInfo::Group"),
                }
            }
            Item::Item(info) => match info {
                TokenInfo::Atom(v) => vec![v],
                TokenInfo::Atoms(vs) => vs,
                _ => panic!("Item::Item should capture TokenInfo::Atom"),
            },
            Item::Placeholder(v) => panic!("cannot convert placeholder {} back to Token", v.name()),
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
        let items = replace_all(&items, &pat, replace);
        items.to_tokens()
    }

    fn find_all(&self, pat: impl ToItems) -> Vec<Match<TokenInfo>> {
        let items = self.to_items();
        let pat = pat.to_items();
        find_all(&items, &pat)
    }

    fn matches_full(&self, pat: impl ToItems) -> Option<Match<TokenInfo>> {
        let items = self.to_items();
        let pat = pat.to_items();
        matches_full(&items, &pat)
    }
}

impl FindReplace for Vec<Item> {
    fn replace_with(&self, pat: impl ToItems, replace: impl Replace<TokenInfo>) -> Self {
        let pat = pat.to_items();
        replace_all(self, &pat, replace)
    }

    fn find_all(&self, pat: impl ToItems) -> Vec<Match<TokenInfo>> {
        let pat = pat.to_items();
        find_all(self, &pat)
    }

    fn matches_full(&self, pat: impl ToItems) -> Option<Match<TokenInfo>> {
        let pat = pat.to_items();
        matches_full(self, &pat)
    }
}

impl AngleBracket for Vec<Item> {
    fn group_by_angle_bracket(self) -> Self {
        let mut iter = self.into_iter();
        let mut result = Vec::new();
        while let Some(item) = iter.next() {
            if matches!(&item, Item::Item(TokenInfo::Atom(TokenTree::Punct(p) )) if p.as_char() == '<')
            {
                // Find the matching '>'
                let mut balance = 1;
                let mut buf = Vec::new();
                // clippy false positive: "iter" cannot be consumed here.
                #[allow(clippy::while_let_on_iterator)]
                while let Some(item2) = iter.next() {
                    if let Item::Item(TokenInfo::Atom(TokenTree::Punct(p))) = &item2 {
                        match p.as_char() {
                            '<' => {
                                balance += 1;
                            }
                            '>' => {
                                balance -= 1;
                            }
                            _ => {}
                        }
                    }
                    if balance == 0 {
                        let (left, right) = match (item, item2) {
                            (Item::Item(TokenInfo::Atom(l)), Item::Item(TokenInfo::Atom(r))) => {
                                (l, r)
                            }
                            _ => unreachable!(),
                        };
                        let info = TokenInfo::CustomGroup(left, right);
                        let tree = Item::Tree(info, buf.group_by_angle_bracket());
                        result.push(tree);
                        break;
                    }
                    buf.push(item2);
                }
            } else {
                result.push(item);
            }
        }
        result
    }
}

impl MatchExt for Match<TokenInfo> {
    fn captured_tokens(&self, name: &str) -> TokenStream {
        match self.captures.get(name) {
            None => TokenStream::new(),
            Some(v) => v.to_tokens(),
        }
    }
}

impl PlaceholderExt for Vec<Item> {
    fn disallow_group_match(self, name: &'static str) -> Self {
        self.with_placeholder_matching_items([(name,
            (|item: &Item| !matches!(item, Item::Tree(TokenInfo::Group(d), _) if *d == Delimiter::Brace))
                as fn(&Item) -> bool,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::parse;
    use crate::prelude::unparse;

    #[test]
    fn test_inseparatable_puncts() {
        let t = |s: &str| {
            let tokens = parse(s);
            let items = tokens.to_items();
            display(&items)
        };
        assert_eq!(t("s: ::String"), "[s, :, ::, String]");
        assert_eq!(t("Result<Vec<u8>>"), "[Result, <, Vec, <, u8, >, >]");
        assert_eq!(
            t("collect::<Result<Vec<_>>>"),
            "[collect, ::, <, Result, <, Vec, <, _, >, >, >]"
        );
        assert_eq!(t("x >>= 2"), "[x, >, >, =, 2]");
        assert_eq!(t("|| -> u8"), "[|, |, ->, u8]");
        assert_eq!(t("1 => 2"), "[1, =>, 2]");
    }

    #[test]
    fn test_group_by_angle_bracket() {
        let tokens = parse("x as Result<Vec<u8>>;");
        let items = tokens.to_items();
        assert_eq!(display(&items), "[x, as, Result, <, Vec, <, u8, >, >, ;]");
        let grouped = items.group_by_angle_bracket();
        assert_eq!(
            display(&grouped),
            "[x, as, Result, Tree(<>, [Vec, Tree(<>, [u8])]), ;]"
        );
        let tokens = grouped.to_tokens();
        assert_eq!(unparse(tokens), "\n            x as Result < Vec < u8 >>;");

        // "->" won't disrupt the grouping.
        let tokens = parse("Box<dyn Fn() -> X>");
        let items = tokens.to_items();
        assert_eq!(
            display(&items),
            "[Box, <, dyn, Fn, Tree(Parenthesis, []), ->, X, >]"
        );
        let grouped = items.group_by_angle_bracket();
        assert_eq!(
            display(&grouped),
            "[Box, Tree(<>, [dyn, Fn, Tree(Parenthesis, []), ->, X])]"
        );
        let tokens = grouped.to_tokens();
        assert_eq!(unparse(tokens), "Box < dyn Fn () -> X >");
    }

    fn display(items: &[Item]) -> String {
        format!("{:?}", items)
            .replace("Item(\"", "")
            .replace("\")", "")
    }
}

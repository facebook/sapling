/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use proc_macro::Delimiter;
use proc_macro::TokenStream;
use proc_macro::TokenTree;

/// TokenTree-like, but:
/// - Supports PartialEq.
/// - Does not contain children for "Group" variant.
///   Instead, children is stored by `Item::<Token>::Tree`.
#[derive(Clone)]
pub(crate) enum TokenInfo {
    Group(Delimiter),
    Atom(TokenTree),
}

impl From<TokenTree> for TokenInfo {
    fn from(value: TokenTree) -> Self {
        match value {
            TokenTree::Group(g) => TokenInfo::Group(g.delimiter()),
            _ => TokenInfo::Atom(value),
        }
    }
}

impl fmt::Debug for TokenInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenInfo::Group(d) => d.fmt(f),
            TokenInfo::Atom(t) => {
                let s = TokenStream::from(t.clone()).to_string();
                write!(f, "{:?}", s)
            }
        }
    }
}

impl PartialEq for TokenInfo {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TokenInfo::Group(l), TokenInfo::Group(r)) => l == r,
            (TokenInfo::Atom(l), TokenInfo::Atom(r)) => match (l, r) {
                (TokenTree::Ident(l), TokenTree::Ident(r)) => l.to_string() == r.to_string(),
                (TokenTree::Literal(l), TokenTree::Literal(r)) => l.to_string() == r.to_string(),
                (TokenTree::Punct(l), TokenTree::Punct(r)) => l.as_char() == r.as_char(),
                _ => false,
            },
            _ => false,
        }
    }
}

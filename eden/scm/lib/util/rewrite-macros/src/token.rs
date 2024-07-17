/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use proc_macro2::Delimiter;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;

/// TokenTree-like, but:
/// - Supports PartialEq. For Group, it only checks the delimiter.
/// - Group inseparate puncts like two ":"s as one item.
/// - Does not contain children for "Group" variant.
///   Instead, children is stored by `Item::<Token>::Tree`.
#[derive(Clone)]
pub(crate) enum TokenInfo {
    Group(Delimiter),
    /// Custom group consists of left and right tokens.
    /// Useful for, `< ... >` (angle bracket).
    CustomGroup(TokenTree, TokenTree),
    Atom(TokenTree),
    /// Inseparable tokens, like "::", "=>", "->".
    /// They use 2 `proc_macro2::TokenTree`s.
    /// But we want to compare them atomically, never match ":" against "::".
    Atoms(Vec<TokenTree>),
}

impl TokenInfo {
    pub(crate) fn from_single(value: TokenTree) -> Self {
        match value {
            TokenTree::Group(g) => TokenInfo::Group(g.delimiter()),
            _ => TokenInfo::Atom(value),
        }
    }

    pub(crate) fn from_multi(values: Vec<TokenTree>) -> Self {
        Self::Atoms(values)
    }
}

impl fmt::Debug for TokenInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenInfo::Group(d) => d.fmt(f),
            TokenInfo::CustomGroup(l, r) => {
                let l = TokenStream::from(l.clone()).to_string();
                let r = TokenStream::from(r.clone()).to_string();
                write!(f, "{l}{r}")
            }
            TokenInfo::Atom(t) => {
                let s = TokenStream::from(t.clone()).to_string();
                write!(f, "{:?}", s)
            }
            TokenInfo::Atoms(ts) => {
                let s = TokenStream::from_iter(ts.iter().cloned()).to_string();
                write!(f, "{:?}", s)
            }
        }
    }
}

impl PartialEq for TokenInfo {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TokenInfo::Group(l), TokenInfo::Group(r)) => l == r,
            (TokenInfo::CustomGroup(l1, r1), TokenInfo::CustomGroup(l2, r2)) => {
                atom_eq(l1, l2) && atom_eq(r1, r2)
            }
            (TokenInfo::Atom(l), TokenInfo::Atom(r)) => atom_eq(l, r),
            (TokenInfo::Atoms(ls), TokenInfo::Atoms(rs)) => {
                ls.len() == rs.len() && ls.iter().zip(rs.iter()).all(|(l, r)| atom_eq(l, r))
            }
            _ => false,
        }
    }
}

fn atom_eq(l: &TokenTree, r: &TokenTree) -> bool {
    match (l, r) {
        (TokenTree::Ident(l), TokenTree::Ident(r)) => l == r,
        (TokenTree::Literal(l), TokenTree::Literal(r)) => l.to_string() == r.to_string(),
        (TokenTree::Punct(l), TokenTree::Punct(r)) => l.as_char() == r.as_char(),
        _ => false,
    }
}

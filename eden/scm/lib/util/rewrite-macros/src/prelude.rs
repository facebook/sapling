/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

pub(crate) use proc_macro2::TokenStream;

pub(crate) use crate::token::TokenInfo;
pub(crate) use crate::token_stream_ext::FindReplace;
pub(crate) use crate::token_stream_ext::ToItems;
pub(crate) use crate::token_stream_ext::ToTokens;
pub(crate) type Item = tree_pattern_match::Item<TokenInfo>;

pub(crate) fn parse(code: &str) -> TokenStream {
    TokenStream::from_str(code).unwrap()
}

pub(crate) fn unparse(stream: impl ToTokens) -> String {
    stream.to_tokens().to_string()
}

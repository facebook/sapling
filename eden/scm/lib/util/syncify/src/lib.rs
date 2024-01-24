/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use std::str::FromStr;

use proc_macro::TokenStream;

mod token;
mod token_stream_ext;

pub(crate) use token::TokenInfo;
use token_stream_ext::TokenStreamExt;
pub(crate) type Item = tree_pattern_match::Item<TokenInfo>;

/// Try to make code non-async by removing `async` and `.await` keywords.
///
/// You can add customized replace logic if the default is not sufficient.
/// For example, use `#[syncify([<B: Future<Output=K>>] => [], [B] => [K])]`
/// to remove `<B: Future<Output=K>>` and replace `B` with `K`. You can also
/// use pattern matching, like`[BoxStream<__1>] => [Iterator<__1>]`.
/// The placeholder names affect what they match:
/// - `__1`: double underscore, without `g`: match a single token that is
///   not a group (not `{ ... }`, `( ... )`).
/// - `__1g`: double underscore, with `g`: match a single token that can
///   also be a group.
/// - `___1`: triple underscore, without `g`: match zero or more tokens,
///   do not match groups.
/// - `___1g`: triple underscore, with `g`: match zero or more tokens,
///   including groups.
///
/// Use `debug` in proc macro attribute to turn on extra output about expanded
/// code. You can also use `cargo expand`.
#[proc_macro_attribute]
pub fn syncify(attr: TokenStream, mut tokens: TokenStream) -> TokenStream {
    let debug = !attr.find_all(parse("debug")).is_empty();
    tokens
        .replace_all(parse(".await"), parse(""))
        .replace_all(parse(".boxed()"), parse(""))
        .replace_all(parse("async move"), parse(""))
        .replace_all(parse("async"), parse(""))
        .replace_all(parse("#[tokio::test]"), parse("#[test]"))
        .replace_all(parse("__::block_on(___g1)"), parse("___g1"));

    // Apply customized replaces.
    let matches = attr.find_all(parse("[___g1] => [___g2]"));
    if debug {
        eprintln!("{} customized replaces", matches.len());
    }
    for m in matches {
        let pat = m.captures.get("___g1").unwrap();
        let replace = m.captures.get("___g2").unwrap();
        tokens.replace_all_raw(pat, replace);
    }

    // `cargo expand` can also be used to produce output.
    if debug {
        eprintln!("output: [[[\n{}\n]]]", unparse(&tokens));
    }

    tokens
}

fn parse(code: &str) -> TokenStream {
    TokenStream::from_str(code).unwrap()
}

fn unparse(stream: &TokenStream) -> String {
    stream.to_string()
}

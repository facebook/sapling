/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

#[allow(unused)]
mod prelude;
mod syncify;
mod token;
mod token_stream_ext;

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
pub fn syncify(
    attr: proc_macro::TokenStream,
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    syncify::syncify(attr.into(), tokens.into()).into()
}

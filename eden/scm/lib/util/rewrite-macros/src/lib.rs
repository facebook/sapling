/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

mod demomo;
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

/// De-monomorphization. Rewrite functions using `impl` parameters like:
///
/// ```
/// #[rewrite_macros::demomo]
/// fn foo(x: impl AsRef<str>) -> String {
///     let x = x.as_ref();
///     x.replace("1", "2").replace("3", "4").replace("5", "6") // complex logic
/// }
/// ```
///
/// to:
///
/// ```ignore
/// fn foo(x: impl AsRef<str>) -> String {
///     fn inner(x: &str) -> String {
///         x.replace("1", "2").replace("3", "4").replace("5", "6") // complex logic
///     }
///     inner(x.as_ref())
/// }
/// ```
///
/// so the complex logic (`inner`) is only compiled once and occurs once in the
/// final binary.
///
/// Supports the following parameters:
/// - `impl AsRef<T>`
/// - `impl Into<T>`
/// - `impl ToString<T>`.
///
/// For functions that take `self`, put `#[demomo]` on the `impl` block
/// so `demomo` can figure out the type of `Self`:
///
/// ```
/// use std::fs;
/// use std::path::Path;
///
/// struct S(String);
/// #[rewrite_macros::demomo]
/// impl S {
///     fn open(path: impl AsRef<Path>) -> Self {
///         Self(fs::read_to_string(path.as_ref()).unwrap())
///     }
///     fn save_as(&self, path: impl AsRef<Path>) {
///         let _ = fs::write(path.as_ref(), self.0.as_bytes());
///     }
///     fn edit(&mut self, content: impl ToString) {
///         self.0 = content.to_string();
///     }
/// }
/// ```
///
/// Use `#[demomo(debug)]` to enable debug output at compile time.
///
/// See also https://matklad.github.io/2021/09/04/fast-rust-builds.html#Compilation-Model-Monomorphization
#[proc_macro_attribute]
pub fn demomo(
    attr: proc_macro::TokenStream,
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    demomo::demomo(attr.into(), tokens.into()).into()
}

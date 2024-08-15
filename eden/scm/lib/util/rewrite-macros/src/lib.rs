/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

mod cached;
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

/// Fill boilerplate of a cached field.
/// The callsite needs to define `OnceCell<Arc<_>>` field. For example:
///
/// ```
/// use std::io::Result;
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use once_cell::sync::OnceCell;
///
/// struct FileReader {
///     path: PathBuf,
///     // Define this field before using `#[cached_field]`!
///     content: OnceCell<Arc<String>>,
/// }
///
/// impl FileReader {
///     pub fn new(path: PathBuf) -> Self {
///         Self {
///             path,
///             content: Default::default(),
///         }
///     }
///
///     #[rewrite_macros::cached_field]
///     pub fn content(&self) -> Result<Arc<String>> {
///         let data = std::fs::read_to_string(&self.path)?;
///         Ok(Arc::new(data))
///     }
/// }
///
/// let dir = tempfile::tempdir().unwrap();
/// let path = dir.path().join("a.txt");
/// let reader = FileReader::new(path.clone());
///
/// std::fs::write(&path, "abc").unwrap();
/// assert_eq!(reader.content().unwrap().as_ref(), "abc");
///
/// // Calling `content()` will use the cache, not read from filesystem again.
/// std::fs::write(&path, "def").unwrap();
/// assert_eq!(reader.content().unwrap().as_ref(), "abc");
/// ```
///
/// If the type is `Arc<RwLock<_>>`, then the cache can be invalidated:
///
/// ```
/// # use std::io::Result;
/// # use std::path::PathBuf;
/// # use std::sync::Arc;
///
/// # use once_cell::sync::OnceCell;
///
/// # struct FileReader {
/// #     path: PathBuf,
/// #     content: OnceCell<Arc<RwLock<String>>>,
/// # }
///
/// # impl FileReader {
/// #     pub fn new(path: PathBuf) -> Self {
/// #         Self {
/// #             path,
/// #             content: Default::default(),
/// #         }
/// #     }
/// # }
///
/// use parking_lot::RwLock;
///
/// impl FileReader {
///     #[rewrite_macros::cached_field]
///     pub fn content(&self) -> Result<Arc<RwLock<String>>> {
///         let data = std::fs::read_to_string(&self.path)?;
///         Ok(Arc::new(RwLock::new(data)))
///     }
/// }
///
/// let dir = tempfile::tempdir().unwrap();
/// let path = dir.path().join("a.txt");
/// let reader = FileReader::new(path.clone());
///
/// std::fs::write(&path, "abc").unwrap();
/// assert_eq!(reader.content().unwrap().read().as_str(), "abc");
///
/// // Cached, stale content.
/// std::fs::write(&path, "def").unwrap();
/// let content = reader.content().unwrap();
/// assert_eq!(content.read().as_str(), "abc");
///
/// // Cache can be invalidated to get the new content.
/// reader.invalidate_content().unwrap();
/// assert_eq!(content.read().as_str(), "def");
/// ```
///
/// To add post processing on the `Arc<RwLock>`, use `post_load`
/// attribute:
///
/// ```
/// # use std::io::Result;
/// # use std::path::PathBuf;
/// # use std::sync::Arc;
///
/// # use once_cell::sync::OnceCell;
///
/// # impl FileReader {
/// #     pub fn new(path: PathBuf) -> Self {
/// #         Self {
/// #             path,
/// #             content: Default::default(),
/// #             backup: Default::default(),
/// #         }
/// #     }
/// # }
///
/// struct FileReader {
///     path: PathBuf,
///     content: OnceCell<Arc<RwLock<String>>>,
///     backup: RwLock<Option<Arc<RwLock<String>>>>,
/// }
///
/// use parking_lot::RwLock;
///
/// impl FileReader {
///     #[rewrite_macros::cached_field(post_load(self.backup_content))]
///     pub fn content(&self) -> Result<Arc<RwLock<String>>> {
///         let data = std::fs::read_to_string(&self.path)?;
///         Ok(Arc::new(RwLock::new(data)))
///     }
///
///     fn backup_content(&self, arc: &Arc<RwLock<String>>) -> Result<()> {
///         *self.backup.write() = Some(Arc::clone(arc));
///         Ok(())
///     }
/// }
/// ```
///
/// Use `#[cached(debug)]` to enable debug output at compile time.
#[proc_macro_attribute]
pub fn cached_field(
    attr: proc_macro::TokenStream,
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    cached::cached_field(attr.into(), tokens.into()).into()
}

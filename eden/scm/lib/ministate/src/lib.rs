/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Track dependencies of values and invalidate/re-calculate accordingly.
//!
//! Inspired by [Jotai](https://jotai.org/). Note this library only implements
//! a small subset of Jotai features.
//!
//! Primitive atom:
//!
//! ```typescript,ignore
//! // Jotai
//! const countAtom = atom(0);
//! store.set(countAtom, 1);
//! ```
//!
//! ```
//! # use std::sync::Arc;
//! # use ministate::atom;
//! # let store = ministate::Store::new();
//! // ministate
//! atom!(CountAtom, u32, 0);
//! store.set::<CountAtom>(1);
//! ```
//!
//! Derived atom:
//!
//! ```typescript,ignore
//! // Jotai
//! const doubledCountAtom = atom((get) => get(countAtom) * 2);
//! const value = store.get(doubledCountAtom);
//! ```
//!
//! ```
//! # use std::sync::Arc;
//! # use ministate::atom;
//! # atom!(CountAtom, u32, 1);
//! # let store = ministate::Store::new();
//! // ministate
//! atom!(DoubledCountAtom, u32, |store| Ok(Arc::new(
//!     *store.get::<CountAtom>()? * 2
//! )));
//! let value = store.get::<DoubledCountAtom>().unwrap();
//! # assert_eq!(*value, 2);
//! # store.set::<CountAtom>(3);
//! # let value = store.get::<CountAtom>().unwrap();
//! # assert_eq!(*value, 3);
//! # let value = store.get::<DoubledCountAtom>().unwrap();
//! # assert_eq!(*value, 6);
//! ```

mod atom;
mod lock;
mod macros;
mod store;

#[cfg(test)]
mod tests;

pub use anyhow::Result;
pub use atom::Atom;
pub use atom::GetAtomValue;
pub use atom::PrimitiveValue;
pub use lock::RwLock;
pub use store::Store;

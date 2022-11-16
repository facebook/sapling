/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// WASM exports must be "pub". But they are not used in this crate.
#![allow(dead_code)]
// Use fooBar naming for exported to match Javascript style.
#![allow(non_snake_case)]

use wasm_bindgen::prelude::*;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

mod convert;
mod dag;
mod drawdag;
mod error;
mod tracing;

pub use convert::Convert;
pub use error::JsError;
pub type JsResult<T> = std::result::Result<T, JsError>;

#[wasm_bindgen(start)]
pub fn setup() {
    ::console_error_panic_hook::set_once();
    ::dag::iddag::set_default_seg_size(3);
}

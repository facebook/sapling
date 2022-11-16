/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Serialize;
use wasm_bindgen::prelude::*;

// Matches std::error::Error.
#[derive(Serialize)]
pub struct JsError(String);

// See https://rustwasm.github.io/wasm-bindgen/reference/types/result.html
impl Into<JsValue> for JsError {
    fn into(self) -> JsValue {
        JsValue::from_str(&self.0)
    }
}

impl<T: std::error::Error> From<T> for JsError {
    fn from(error: T) -> Self {
        to_js_error(&error)
    }
}

fn to_js_error(error: &dyn std::error::Error) -> JsError {
    let mut message = error.to_string();
    if let Some(inner_error) = error.source() {
        message += &format!(" ({})", to_js_error(inner_error).0);
    }
    JsError(message)
}

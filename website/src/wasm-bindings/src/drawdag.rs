/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use dag::Vertex;
use wasm_bindgen::prelude::*;

use crate::Convert;

/// ASCII graph -> {name: List[name]}
#[wasm_bindgen]
pub fn drawdag(text: &str) -> JsValue {
    let parent_map: BTreeMap<String, BTreeSet<String>> = drawdag::parse(text);
    let parent_map: BTreeMap<Vertex, Vec<Vertex>> = parent_map.convert();
    // drawdag does not prevent cycles. Break cycles here to avoid infinite loops.
    let parent_func = dag::utils::break_parent_func_cycle(|name: Vertex| {
        Ok(parent_map.get(&name).cloned().unwrap_or_default())
    });
    let result: BTreeMap<Vertex, Vec<Vertex>> = parent_map
        .keys()
        .map(|k| (k.clone(), parent_func(k.clone()).unwrap()))
        .collect();
    let result: BTreeMap<String, Vec<String>> = result.convert();
    serde_wasm_bindgen::to_value(&result).unwrap()
}

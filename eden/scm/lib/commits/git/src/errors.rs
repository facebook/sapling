/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag::Vertex;

pub fn hash_mismatch(a: &Vertex, b: &Vertex) -> anyhow::Error {
    anyhow::format_err!("hash mismatch: {:?} != {:?}", a, b)
}

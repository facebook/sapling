/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Vertex;

pub fn hash_mismatch(a: &Vertex, b: &Vertex) -> anyhow::Error {
    anyhow::format_err!("hash mismatch: {:?} != {:?}", a, b)
}

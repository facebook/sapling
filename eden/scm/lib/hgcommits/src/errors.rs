/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Vertex;

pub fn test_only(name: &str) -> anyhow::Error {
    anyhow::format_err!("{} should only be used in tests", name)
}

pub fn hash_mismatch(a: &Vertex, b: &Vertex) -> anyhow::Error {
    anyhow::format_err!("hash mismatch: {:?} != {:?}", a, b)
}

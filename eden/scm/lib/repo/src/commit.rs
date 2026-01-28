/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;

use format_util::HgTime;
use types::HgId;
pub struct Commit {
    pub manifest: HgId,
    pub files: Vec<String>,
    pub description: String,
    pub user: String,
    pub date: Option<HgTime>,
    pub extra: BTreeMap<String, String>,
    pub parents: Vec<HgId>,
    pub gpg_keyid: Option<String>,
}

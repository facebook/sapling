/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use format_util::HgCommitFields;
use serde::Deserialize;
use types::HgId;

#[derive(Deserialize)]
pub struct NewCommit {
    pub commit_fields: HgCommitFields,
    #[serde(default)]
    pub parents: Vec<HgId>,
    #[serde(default)]
    pub gpg_keyid: Option<String>,
}

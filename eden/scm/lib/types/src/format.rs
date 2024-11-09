/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

/// Decides the serialization format. This exists so different parts of the code
/// base can agree on how to generate a SHA1, how to lookup in a tree, etc.
/// Ideally this information is private and the differences are behind
/// abstractions too.
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Default,
    Serialize,
    Deserialize
)]
#[serde(rename_all = "snake_case")]
pub enum SerializationFormat {
    // Hg SHA1:
    //   SORTED_PARENTS CONTENT
    //
    // Hg file:
    //   FILELOG_METADATA CONTENT
    //
    // Hg tree:
    //   NAME '\0' HEX_SHA1 MODE '\n'
    //   MODE: 't' (tree), 'l' (symlink), 'x' (executable)
    //   (sorted by name)
    #[default]
    Hg,

    // Git SHA1:
    //   TYPE LENGTH CONTENT
    //
    // Git file:
    //   CONTENT
    //
    // Git tree:
    //   MODE ' ' NAME '\0' BIN_SHA1
    //   MODE: '40000' (tree), '100644' (regular), '100755' (executable),
    //         '120000' (symlink), '160000' (gitlink)
    //   (sorted by name, but directory names are treated as ending with '/')
    Git,
}

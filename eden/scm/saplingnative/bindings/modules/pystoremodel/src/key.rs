/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;
use types::Id20;
use types::Key;
use types::RepoPathBuf;

/// `types::Key` but in the compact form.
/// Accepts `(path, node)` format from Python, instead of the more verbose
/// `{'path': path, 'node': node}`.
#[derive(Serialize, Deserialize)]
pub struct CompactKey(RepoPathBuf, Id20);

impl CompactKey {
    pub(crate) fn into_key(self) -> Key {
        Key::new(self.0, self.1)
    }

    pub(crate) fn from_key(key: Key) -> CompactKey {
        Self(key.path, key.hgid)
    }
}

// Work around Rust's orphan rule
pub(crate) trait IntoKeys {
    fn into_keys(self) -> Vec<Key>;
}

impl IntoKeys for Vec<CompactKey> {
    fn into_keys(self) -> Vec<Key> {
        self.into_iter().map(|k| k.into_key()).collect()
    }
}

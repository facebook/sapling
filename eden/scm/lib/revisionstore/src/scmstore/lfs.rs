/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use tracing::error;

use configparser::convert::ByteCount;

use crate::indexedlogdatastore::Entry;

pub fn lfs_threshold_filtermap_fn(lfs_threshold: ByteCount) -> impl Fn(Entry) -> Option<Entry> {
    move |mut entry: Entry| {
        if entry.metadata().is_lfs() {
            None
        } else {
            match entry.content() {
                Ok(content) => {
                    if content.len() > lfs_threshold.value() as usize {
                        None
                    } else {
                        Some(entry)
                    }
                }
                Err(e) => {
                    // TODO(meyer): This is safe, but is it correct? Should we make the filter_map fn fallible instead?
                    // If we failed to read `content`, reject the write.
                    error!({ error = %e }, "error reading entry content for LFS threshold check");
                    None
                }
            }
        }
    }
}

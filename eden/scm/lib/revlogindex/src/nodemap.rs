/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::u32;

use radixbuf::key::KeyId;
use radixbuf::radix::radix_insert;
use radixbuf::radix::radix_lookup;
use radixbuf::radix::radix_lookup_unchecked;
use radixbuf::radix::radix_prefix_lookup;
use radixbuf::radix::RADIX_NCHILDREN;

use crate::errors::corruption;
use crate::Result;
use crate::RevlogEntry;

/// An index for node to rev lookups.
///
/// The index depends entirely on an append-only changelog.i source
/// of truth. It does not support in-memory overrides, which could be
/// implemented at a higher level.
///
/// ```text
///
///     changelogi
///   +------------+
///   | ... | node | < rev 0  \
///   +------------+           |
///   | ... | node | < rev 1   |> included in the main (on-disk) index
///   +------------+           |
///   | .......... | ...      /
///   +------------+
///   | ... | node | < next_index_rev         \
///   +------------+                           |
///   | ... | node | < next_index_rev + 1      |  will be built on-demand
///   +------------+                           |> in the side (in-memory)
///   | .......... | ...                       |  index
///   +------------+                           |
///   | ... | node | < next_changelog_rev - 1 /
///   +------------+
///                  < next_changelog_rev
/// ```
///
/// The main index is an immutable, periodically-rebuilt, on-disk radix buffer
/// with an extra metadata about what's the next revision unknown to the index.
/// The side index covers remaining revisions in changelogi, built on-demand and
/// is in-memory only. The side index is usually much smaller than the main index
/// so it can be built quickly.
///
/// ```text
///         main index               side index
///   +---------------------+  +----------------------+
///   | next_index_rev: u32 |  | (small radix buffer) |
///   +---------------------+  +----------------------+
///   |                     |      (in-memory only)
///   |(large radix buffer) |
///   |                     |
///   +---------------------+
///    (backed by filesystem)
/// ```
///
/// Having the side index allows us to make the main index immutable for most
/// of the time even if the source of truth has changed. It's possible to update
/// the main index in-place. But that requires extra efforts to deal with possible
/// filesystem issues like locking, or unexpected poweroff.
#[derive(Clone)]
pub struct NodeRevMap<C, I> {
    pub(crate) changelogi: C,
    main_index: I,             // Immutable main index
    side_index: Arc<Vec<u32>>, // Mutable side index
}

// Offsets in the main radix and key buffers
const RADIX_NEXT_REV_OFFSET: usize = 0;
const RADIX_HEADER_LEN: usize = RADIX_NEXT_REV_OFFSET + 1;

// Offsets of root nodes in radix buffers
const MAIN_RADIX_OFFSET: u32 = 1;
const SIDE_RADIX_OFFSET: u32 = 0;

impl<C: AsRef<[RevlogEntry]>, I: AsRef<[u32]>> NodeRevMap<C, I> {
    /// Initialize NodeMap from a non-inlined version of changelog.i and an incomplete index.
    pub fn new(changelogi: C, main_index: I) -> Result<Self> {
        // Sanity check if the index is corrupted or not.

        // The index must contain at least 17 elements. index[0] tracks the last rev the index has.
        // index[1..17] is the root radix node.
        if main_index.as_ref().len() < RADIX_HEADER_LEN + RADIX_NCHILDREN {
            return corruption("revlog radix index corrupted (main index too small)");
        }

        // Check if the index is behind and build incrementally
        let next_rev = u32::from_be(main_index.as_ref()[RADIX_NEXT_REV_OFFSET]);
        let end_rev = changelog_end_rev(&changelogi);

        if next_rev > end_rev {
            // next_rev cannot be larger than what changelogi has.
            return corruption("revlog radix index corrupted (next_rev > end_rev)");
        } else if next_rev > 0 {
            // Sanity check: if the last node stored in the index does not match the changelogi,
            // the index is broken and needs rebuilt. That could happen if strip happens.
            let rev: KeyId = (next_rev - 1).into();
            let node = rev_to_node(&changelogi, rev)?;
            if let Ok(Some(id)) = radix_lookup_unchecked(&main_index, MAIN_RADIX_OFFSET, &node) {
                if id != rev {
                    return corruption("revlog radix index corrupted (revlog out-of-sync)");
                }
            } else {
                return corruption("revlog radix index corrupted (revlog out-of-sync)");
            }
        }

        // Build side_index for the revisions not in the main index
        let mut side_index = vec![0u32; RADIX_NCHILDREN];
        build(
            &changelogi,
            &mut side_index,
            SIDE_RADIX_OFFSET,
            next_rev,
            end_rev,
        )?;
        let side_index = Arc::new(side_index);

        Ok(NodeRevMap {
            changelogi,
            main_index,
            side_index,
        })
    }

    /// Convert hex prefix to node.
    pub fn hex_prefix_to_node<T: AsRef<[u8]>>(&self, hex_prefix: T) -> Result<Option<&[u8]>> {
        let bin_prefix = match hex_to_bin_base16(hex_prefix) {
            Some(v) => v,
            None => return Ok(None),
        };
        let iter = bin_prefix.iter().cloned();
        let cl = &self.changelogi;
        let main_res = radix_prefix_lookup(
            &self.main_index,
            MAIN_RADIX_OFFSET,
            iter.clone(),
            rev_to_node,
            cl,
        )?;
        let side_res = radix_prefix_lookup(
            self.side_index.as_ref(),
            SIDE_RADIX_OFFSET,
            iter,
            rev_to_node,
            cl,
        )?;
        match (main_res, side_res) {
            (Some(_), Some(_)) => Err(crate::Error::AmbiguousPrefix),
            (Some(rev), None) | (None, Some(rev)) => Ok(Some(rev_to_node(&self.changelogi, rev)?)),
            _ => Ok(None),
        }
    }

    /// Convert node to rev.
    pub fn node_to_rev<T: AsRef<[u8]>>(&self, node: T) -> Result<Option<u32>> {
        let cl = &self.changelogi;
        if let Some(rev) = radix_lookup(&self.main_index, 1, &node, rev_to_node, cl)? {
            Ok(Some(rev.into()))
        } else if let Some(rev) = radix_lookup(self.side_index.as_ref(), 0, &node, rev_to_node, cl)?
        {
            Ok(Some(rev.into()))
        } else {
            Ok(None)
        }
    }

    /// How many revisions the side index has.
    pub fn lag(&self) -> u32 {
        let next_rev = u32::from_be(self.main_index.as_ref()[0]);
        let end_rev = changelog_end_rev(&self.changelogi);
        end_rev - next_rev
    }

    /// Incrementally build the main index based on the existing one.
    /// Note: this will memcpy the immutable main index so the new buffer
    /// could be written and resized.
    pub fn build_incrementally(&self) -> Result<Vec<u32>> {
        // Copy the main index since we need to modify it.
        let mut index = self.main_index.as_ref().to_vec();
        let end_rev = changelog_end_rev(&self.changelogi);
        let next_rev = u32::from_be(index[0]);
        build(
            &self.changelogi,
            &mut index,
            MAIN_RADIX_OFFSET,
            next_rev,
            end_rev,
        )?;
        index[0] = end_rev.to_be();
        Ok(index)
    }
}

/// Return the minimal revision number the changelog.i does not have.
fn changelog_end_rev<T: AsRef<[RevlogEntry]>>(changelogi: &T) -> u32 {
    let changelogi = changelogi.as_ref();
    let rev = changelogi.len();
    if rev as u64 > u32::MAX as u64 {
        panic!("rev exceeds 32 bit integers")
    }
    rev as u32
}

/// Read the given range of revisions (from `start_rev` (inclusive) to
/// `end_rev` (exclusive)) from changelogi. Insert them to the radix
/// index.
fn build<T>(
    changelogi: &T,
    index: &mut Vec<u32>,
    radix_offset: u32,
    start_rev: u32,
    end_rev: u32,
) -> Result<()>
where
    T: AsRef<[RevlogEntry]>,
{
    // Reserve the approximate size needed for the index - 28 bytes for each revision.
    // See D1291 for a table of number of revisions and index sizes.
    index.reserve(7 * (end_rev - start_rev) as usize);
    for i in start_rev..end_rev {
        let _ = radix_insert(index, radix_offset, i.into(), rev_to_node, changelogi)?;
    }
    Ok(())
}

/// Helper method similar to `radixbuf::key::FixedKey::read`, but takes a revision number instead.
fn rev_to_node<K: AsRef<[RevlogEntry]>>(changelogi: &K, rev: KeyId) -> radixbuf::Result<&[u8]> {
    let buf = changelogi.as_ref();
    let rev_usize: usize = rev.into();
    let entry = &buf[rev_usize];
    Ok(&entry.node[..])
}

/// Convert hex base16 sequence to binary base16 sequence.
fn hex_to_bin_base16<T: AsRef<[u8]>>(base16: T) -> Option<Vec<u8>> {
    let base16 = base16.as_ref();
    let len = base16.len();
    let mut result = vec![0u8; len];
    for (i, &ch) in base16.iter().enumerate() {
        result[i] = match ch {
            b'a'..=b'f' => ch - b'a' + 10,
            b'A'..=b'F' => ch - b'A' + 10,
            b'0'..=b'9' => ch - b'0',
            _ => return None,
        }
    }
    Some(result)
}

/// Return an empty index that can be used as "main_index" when passed to `new`.
pub fn empty_index_buffer() -> Vec<u8> {
    return vec![0u8; 4 * (RADIX_HEADER_LEN + RADIX_NCHILDREN)];
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Blob store on local disk.
//!
//! See [Zstore] for the main structure.

use crypto::digest::Digest;
use crypto::sha1::Sha1;
use indexedlog::log as ilog;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use tracing::{debug_span, info_span, trace_span};
pub use types::Id20;

/// An append-only local-disk blob storage.
///
/// Blobs are addressed by their SHA1 digest, compressed using zstd algorithm.
/// Blobs with similar contents can be stored as a chain of zstd dictionary
/// compression results to save space. That is, a newer version is compressed
/// using an existing version as a zstd dictionary.
///
/// The name `Zstore` was chosen because the prefix `zst` is the name of the
/// compression algorithm.
pub struct Zstore {
    dir: PathBuf,
    log: ilog::Log,
    pub delta_opts: DeltaOptions,
}

impl Zstore {
    const ID20_INDEX: usize = 0;

    /// Load or create [Zstore] at the given directory.
    pub fn open(dir: impl AsRef<Path>) -> crate::Result<Zstore> {
        let dir = dir.as_ref();
        let log = ilog::OpenOptions::new()
            .index("id", |_| -> Vec<_> {
                // The offset of `start` should match mincode serialization layout
                // of `Delta`.
                let start = 0;
                let end = start + Id20::len();
                vec![ilog::IndexOutput::Reference(start as u64..end as u64)]
            })
            .create(true)
            .flush_filter(Some(|context, data| {
                // At flush time, there might be data written by other processes.
                // Drop duplicated entries.
                let id = &data[0..Id20::len()];
                if let Ok(mut iter) = context.log.lookup(Self::ID20_INDEX, id) {
                    if iter.nth(0).is_some() {
                        return Ok(ilog::FlushFilterOutput::Drop);
                    }
                }
                Ok(ilog::FlushFilterOutput::Keep)
            }))
            .open(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            log,
            delta_opts: Default::default(),
        })
    }

    /// Insert a new blob to the store. Return its identity (SHA1).
    ///
    /// `candidate_base_ids` can be used to provide a list of "similar" blob
    /// identities that can be used as zstd dictionaries to achieve better
    /// compression. Note: delta base outside the candidate list can also be
    /// chosen to optimize the delta chain length.
    ///
    /// Writes are buffered. Call [Zstore::flush] to actually write them to
    /// disk. See [indexedlog::Log::append] and [indexedlog::Log::sync] for
    /// details.
    pub fn insert(&mut self, data: &[u8], candidate_base_ids: &[Id20]) -> crate::Result<Id20> {
        let id = sha1(data);

        if self.contains(id)? {
            return Ok(id);
        }

        debug_span!(
            "Zstore::insert",
            data_len = data.len(),
            id = &AsRef::<str>::as_ref(&id.to_hex())
        )
        .in_scope(|| {
            // Base line: delta against b"".
            let compressed = zstdelta::diff(b"", data)?;
            let chain_bytes = compressed.len();
            let mut best_delta = Delta {
                id,
                base_id: *EMPTY_ID20,
                depth: 1, // "EMPTY" has depth 0
                subchain_len: 0,
                chain_bytes,
                data: Cow::Owned(compressed),
            };

            // Attempt to use delta bases.
            for &base_id in candidate_base_ids {
                if let Some(delta) = self.create_delta(id, base_id, data, false)? {
                    if delta.data.len() < best_delta.data.len() {
                        best_delta = delta;
                    }
                }
            }

            // Insert the delta to the blob store.
            let bytes = mincode::serialize(&best_delta)?;
            self.log.append(bytes)?;
            Ok(id)
        })
    }

    /// Write blobs to disk.
    ///
    /// See [indexedlog::Log::sync] for details.
    /// Return the size of the main log in bytes.
    pub fn flush(&mut self) -> crate::Result<u64> {
        info_span!("Zstore::flush").in_scope(|| Ok(self.log.flush()?))
    }

    /// Get the content of the specified blob.
    pub fn get(&self, id: Id20) -> crate::Result<Option<Vec<u8>>> {
        debug_span!("Zstore::get", id = &AsRef::<str>::as_ref(&id.to_hex())).in_scope(|| match self
            .get_delta(id)?
        {
            None => Ok(None),
            Some(delta) => Ok(Some(self.resolve(delta)?)),
        })
    }

    /// Check if the store contains the given blob identity.
    pub fn contains(&self, id: Id20) -> crate::Result<bool> {
        debug_span!("Zstore::contains", id = &AsRef::<str>::as_ref(&id.to_hex())).in_scope(|| {
            let mut results = self.log.lookup(Self::ID20_INDEX, id)?;
            Ok(results.next().transpose().map(|v| v.is_some())?)
        })
    }

    /// Create a new [`Delta`] using the specified delta base candidate.
    /// Satisfy limitations specified by [`DeltaOptions`].
    /// Return `None` if a suitable delta cannot be created.
    fn create_delta<'a>(
        &'a self,
        id: Id20,
        base_id: Id20,
        data: &'a [u8],
        mut preserve_depth: bool,
    ) -> crate::Result<Option<Delta<'a>>> {
        if base_id == *EMPTY_ID20 {
            // Pointless to create a delta against EMPTY_ID20.
            return Ok(None);
        }

        let base_delta = match self.get_delta(base_id)? {
            Some(v) => v,
            None => {
                // base_id does not exist.
                return Ok(None);
            }
        };

        // See docstring above Delta.depth for how this works.
        if base_delta.depth >= self.delta_opts.max_depth {
            // Cannot go deeper.
            preserve_depth = true;
        }

        if preserve_depth && base_delta.subchain_len + 1 >= self.delta_opts.max_subchain_len {
            // Creating a delta based on base_id will exceed the max_subchain_len
            // limit. So attempt to use a "parent" delta instead.
            let current_depth = base_delta.depth;
            let mut current_parent = base_delta;
            while let Some(parent) = self.get_delta(current_parent.base_id)? {
                if parent.id == *EMPTY_ID20 {
                    // Avoid infinite loop.
                    return Ok(None);
                }
                if parent.depth < current_depth {
                    // Find a parent. But it can also exceeds the
                    // max_subchain_len limit. So call create_delta
                    // recursively.
                    return self.create_delta(id, parent.id, data, true);
                } else {
                    current_parent = parent
                }
            }
            Ok(None)
        } else {
            let (depth, subchain_len) = if preserve_depth {
                // Create a new delta at the current depth.
                (base_delta.depth, base_delta.subchain_len + 1)
            } else {
                // Haven't reached the maximum delta depth. So attempt to
                // increase the delta depth here.
                (base_delta.depth + 1, 0)
            };
            let base_bytes = base_delta.chain_bytes;
            let bytes = zstdelta::diff(&self.resolve(base_delta)?, data)?;
            let chain_bytes = base_bytes + bytes.len();
            if chain_bytes
                > self
                    .delta_opts
                    .max_chain_bytes
                    .min(data.len() << self.delta_opts.max_chain_factor_log)
            {
                // The delta length is not suitable for a delta.
                Ok(None)
            } else {
                // Create a new delta at the next depth.
                Ok(Some(Delta {
                    id,
                    base_id,
                    depth,
                    subchain_len,
                    chain_bytes,
                    data: Cow::Owned(bytes),
                }))
            }
        }
    }

    /// Decode a [`Delta`]. Do not apply delta chain to get full text.
    fn get_delta<'a>(&'a self, id: Id20) -> crate::Result<Option<Delta<'a>>> {
        if id == *EMPTY_ID20 {
            return Ok(Some(Delta {
                id,
                base_id: id,
                depth: 0,
                subchain_len: 0,
                chain_bytes: 0,
                data: Cow::Borrowed(b""),
            }));
        }
        let mut results = self.log.lookup(0, id)?;
        match results.next() {
            None => Ok(None),
            Some(Ok(bytes)) => {
                let result = mincode::deserialize(bytes)?;
                Ok(Some(result))
            }
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Apply delta chains recursively to reconstruct full text.
    fn resolve<'a>(&'a self, delta: Delta<'a>) -> crate::Result<Vec<u8>> {
        if delta.id == *EMPTY_ID20 {
            return Ok(Vec::new());
        }

        trace_span!(
            "Zstore::resolve",
            id = &AsRef::<str>::as_ref(&delta.id.to_hex()),
            base_id = &AsRef::<str>::as_ref(&delta.base_id.to_hex()),
            depth = delta.depth,
            subchain_len = delta.subchain_len,
            chain_bytes = delta.chain_bytes,
            data_len = delta.data.len(),
        )
        .in_scope(|| {
            match self.get_delta(delta.base_id)? {
                Some(base_delta) => {
                    // PERF: some caching would avoid N^2 chain application.
                    let base_bytes = self.resolve(base_delta)?;
                    Ok(zstdelta::apply(&base_bytes, &delta.data)?)
                }
                None => Err(self.error(format!(
                    "incomplete delta chain: {} -> {} chain is broken",
                    delta.id.to_hex(),
                    delta.base_id.to_hex()
                ))),
            }
        })
    }

    fn error(&self, message: impl fmt::Display) -> crate::Error {
        crate::Error(format!("{:?}: {}", &self.dir, message))
    }
}

// -------- Utilities --------

pub fn sha1(data: &[u8]) -> Id20 {
    trace_span!("sha1", data_len = data.len()).in_scope(|| {
        let mut hasher = Sha1::new();
        hasher.input(data);
        let mut id = [0u8; 20];
        hasher.result(&mut id);
        Id20::from_byte_array(id)
    })
}

// -------- Options --------

/// Options for deltas.
///
/// `max_depth` and `max_subchain_len` reshapes linear deltas to a tree,
/// reducing delta-chain length.
///
/// For example, the following graph shows 1 full text and 12 deltas.
/// The maximum chain length is 6. If those 12 deltas are chained
/// linearly, the maximum chain length would be 12.
///
/// ```plain,ignore
/// Tree            | Base  | Parent | Depth | Chain        | Sub Chain
/// --------------------------------------------------------------------
/// Rev 0           | EMPTY | -      | 0     |              |
///  +-- Rev 1      | Rev 0 | Rev 0  | 1     | 1            | 1
///  |    +-- Rev 2 | Rev 1 | Rev 1  | 2     | 1-2          | 2
///  |    +-- Rev 3 | Rev 2 | Rev 1  | 2     | 1-2-3        | 2-3
///  |    +-- Rev 4 | Rev 3 | Rev 1  | 2     | 1-2-3-4      | 2-3-4
///  +-- Rev 5      | Rev 1 | Rev 0  | 1     | 1-5          | 1-5
///  |    +-- Rev 6 | Rev 5 | Rev 5  | 2     | 1-5-6        | 6
///  |    ...       |       |        |       |              |
///  +-- Rev 9      | Rev 5 | Rev 0  | 1     | 1-5-9        | 9
///       ...       |       |        |       |              |
///       +-- Rev 12| Rev 9 | Rev 11 | 2     |1-5-9-10-11-12| 10-11-12
///                   ^       ^        ^       ^              ^
///                   |       |        |       |         revs in same depth
///                   |       |        |       see "Tree", path from root to
///                   |       |        |       rev, including siblings
///                   |       |        see "Tree"
///                   |       rev with one less depth
///                   delta base
///
/// In this example, `max_depth` is 2, `max_subchain_len` is 3.
/// ```
///
/// Let `d` be `max_depth`, `n` be `max_subchain_len`, this would rewrite a
/// linear chain of `Sum[n**i, {i, 1, d}]` (evaluates to
/// `(n * (n ** d - 1)) / (n - 1)`) deltas into a tree with a maximum chain
/// length `n * d`.
///
/// Table of linear chain length (i.e. a linear chain of such length can
/// be reshaped without using full texts):
///
/// ```plain,ignore
///     |    d=2      3      4      5      6      7
/// -----------------------------------------------
/// n=2 |      6     14     30     62    126    254
///   3 |     12     39    120    363   1092   3279
///   4 |     20     84    340   1364   5460  21844
///   5 |     30    155    780   3905  19530  97655
///   6 |     42    258   1554   9330  55986 335922
///   7 |     56    399   2800  19607 137256 960799
/// ```
///
/// Table of average reshaped chain length:
///
/// ```plain,ignore
///     |    d=2      3      4      5      6      7
/// -----------------------------------------------
/// n=2 |    2.5    3.6    4.9    6.2    7.6    9.1
///   3 |    3.5    5.2    7.1    9.0   11.0   13.0
///   4 |    4.5    6.8    9.2   11.7   14.2   16.7
///   5 |    5.5    8.3   11.3   14.3   17.3   20.3
///   6 |    6.5    9.8   13.3   16.8   20.3   23.8
///   7 |    7.5   11.4   15.3   19.3   23.3   27.3
/// ```
///
/// In general, a larger `n` helps space usage for shorter chains,
/// a larger `d` helps handling longer chains.
pub struct DeltaOptions {
    /// Maximum depth of a delta.
    ///
    /// See [`DeltaOptions`] for explanation.
    pub max_depth: usize,

    /// Maximum length of a subchain.
    ///
    /// See [`DeltaOptions`] for explanation.
    pub max_subchain_len: usize,

    /// Maximum bytes for the total compressed bytes in the delta chain.
    pub max_chain_bytes: usize,

    /// Do not use delta, if the total compressed bytes in the delta chain
    /// exceeds `uncompressed_source_data.len() << max_chain_factor_log`.
    pub max_chain_factor_log: u8,

    /// Prevent constructing this struct.
    _private: (),
}

impl Default for DeltaOptions {
    fn default() -> Self {
        DeltaOptions {
            max_depth: 5,
            max_subchain_len: 4,
            max_chain_bytes: 500_000_000,
            // Stop using delta if the compressed chain exceeds
            // 2x uncompressed full text size. This matches
            // mercurial revlog behavior.
            max_chain_factor_log: 1,
            _private: (),
        }
    }
}

lazy_static! {
    static ref EMPTY_ID20: Id20 = sha1(b"");
}

// -------- Serde Structures --------

#[derive(Serialize, Deserialize)]
struct Delta<'a> {
    // ATTENTION: The 0..20 byte slice of mincode serialized Delta is expected to be the id.
    /// Pre-calculated checksum of the full data.
    id: Id20,

    /// Delta base Id. Use `EMPTY_ID20` if there is no real delta base.
    base_id: Id20,

    /// Depth of the delta. See [`DeltaOptions`] for how this works.
    depth: usize,

    /// Count of deltas with the same depth in the current chain, excluding
    /// self.
    ///
    /// ```plain,ignore
    /// Depth:         0  1  1  1  2  2  2
    /// Chain:         _                   Depth 0
    ///                 \-o--o--o          Depth 1 (subchain)
    ///                          \-o--o--o Depth 2 (subchain)
    /// Chain Len:     0  1  2  3  4  5  6
    /// Subchain Len:  0  0  1  2  0  1  2
    /// ```
    ///
    /// See also `depth`.
    ///
    /// If `subchain_len` reaches the `max_subchain_len` limit. This delta will
    /// no longer be a delta base candidate, its parent (the nearest delta in
    /// the chain with a smaller depth) will be considered instead.
    subchain_len: usize,

    /// Delta chain size in total. Help cutting down a delta chain.
    chain_bytes: usize,

    /// Delta content.
    ///
    /// Output of [`zstdelta::diff`] (owned) or [`mincode::deserialize`]
    /// (borrowed).
    #[serde(borrow)]
    data: Cow<'a, [u8]>,
}

// -------- Tests --------

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use tempfile::TempDir;

    #[test]
    fn test_simple_case() {
        let contents = [
            &b"11111111111111111111111111111111"[..],
            &b"1111111111111111111111111111112"[..],
            &b"111111111111111111111111111113"[..],
            &b""[..],
        ];
        test_round_trips(&contents);
    }

    #[test]
    fn test_dedup_on_flush() {
        let dir = TempDir::new().unwrap();
        let mut zstore = Zstore::open(&dir).unwrap();
        let id = zstore.insert(b"123456", &[]).unwrap();
        zstore.flush().unwrap();

        let mut zstore1 = Zstore::open(&dir).unwrap();
        let mut zstore2 = Zstore::open(&dir).unwrap();
        let id1 = zstore1.insert(b"1234567", &[id]).unwrap();
        let id2 = zstore2.insert(b"1234567", &[]).unwrap();
        assert_eq!(id1, id2);

        // Because id1 == id2, they will be de-duplicated by the flush_filter
        // on ilog::Log::flush().
        let size1 = zstore1.flush().unwrap();
        let size2 = zstore2.flush().unwrap();
        assert_eq!(size1, size2);
    }

    quickcheck! {
        fn test_random_round_trips(contents: Vec<Vec<u8>>) -> bool {
            test_round_trips(&contents);
            true
        }
    }

    fn test_round_trips(contents: &[impl AsRef<[u8]>]) {
        let dir = TempDir::new().unwrap();
        let mut zstore = Zstore::open(&dir).unwrap();

        let mut ids: Vec<Id20> = Vec::new();
        for content in contents.iter() {
            let mut base_ids: Vec<Id20> = Vec::new();
            if !ids.is_empty() {
                let pseudo_rand = AsRef::<[u8]>::as_ref(&ids[ids.len() - 1])[0] as usize + 1;
                base_ids.push(ids[(ids.len() - 1usize) % pseudo_rand]);
            }
            let id = zstore.insert(content.as_ref(), &base_ids).unwrap();
            ids.push(id);
        }

        for (i, id) in ids.iter().enumerate() {
            assert_eq!(zstore.get(*id).unwrap().unwrap(), contents[i].as_ref());
        }

        zstore.flush().unwrap();
        let zstore = Zstore::open(&dir).unwrap();
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(zstore.get(*id).unwrap().unwrap(), contents[i].as_ref());
        }
    }
}

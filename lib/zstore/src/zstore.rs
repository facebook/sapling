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

// -------- Debug APIs --------

/// Represent delta relationships.
pub struct DebugDeltaTree {
    id: Id20,
    len: usize,
    chain_len: usize,
    depth: usize,
    subchain_len: usize,
    children: Vec<DebugDeltaTree>,
}

#[allow(dead_code)]
impl Zstore {
    /// Reconstruct a tree of deltas for debugging purpose.
    fn debug_delta_tree(&self) -> crate::Result<DebugDeltaTree> {
        let mut root = DebugDeltaTree {
            id: *EMPTY_ID20,
            len: 0,
            chain_len: 0,
            depth: 0,
            subchain_len: 0,
            children: Vec::new(),
        };

        fn insert<'a>(tree: &'a mut DebugDeltaTree, delta: Delta) -> &'a mut DebugDeltaTree {
            let id = tree
                .children
                .iter()
                .enumerate()
                .find(|(_, c)| c.id == delta.id)
                .map(|(id, _)| id);
            match id {
                Some(id) => &mut tree.children[id],
                None => {
                    tree.children.push(DebugDeltaTree {
                        id: delta.id,
                        len: delta.data.len(),
                        chain_len: tree.chain_len + 1,
                        depth: delta.depth,
                        subchain_len: delta.subchain_len,
                        children: Vec::new(),
                    });
                    tree.children.last_mut().unwrap()
                }
            }
        }

        for entry in self.log.iter() {
            let id = &self.log.index_func(Self::ID20_INDEX, entry?)?[0];
            let mut id = Id20::from_slice(id).unwrap();
            let mut chain: Vec<Delta> = Vec::new();
            while id != *EMPTY_ID20 {
                if let Some(delta) = self.get_delta(id)? {
                    id = delta.base_id;
                    chain.push(delta);
                } else {
                    return Err(crate::Error(format!(
                        "unexpected broken chain around {}",
                        id.to_hex()
                    )));
                }
            }

            let mut tree = &mut root;
            for delta in chain.into_iter().rev() {
                tree = insert(tree, delta);
            }
        }

        Ok(root)
    }
}

impl fmt::Debug for DebugDeltaTree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.id == *EMPTY_ID20 {
            // Write header.
            write!(f, "Chain Len| Depth |Subchain Len| Bytes | Chain ID\n")?;
        }
        write!(
            f,
            "{:8} | {:5} | {:10} | {:5} | {}{}\n",
            self.chain_len,
            self.depth,
            self.subchain_len,
            self.len,
            " ".repeat(self.chain_len),
            &self.id.to_hex()[..6],
        )?;
        for child in self.children.iter() {
            write!(f, "{:?}", child)?;
        }
        Ok(())
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

    /// Generate noise that is hard to compress.
    fn generate_noise(approximated_len: usize) -> String {
        (0..(approximated_len / 41))
            .map(|i| sha1(&[(i % 100) as u8, (i % 101) as u8][..]).to_hex())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Adjust `DeltaOptions`. Insert `contents` as linear delta chain.
    /// Print the delta tree to a `String`.
    fn show_tree(contents: &[impl AsRef<[u8]>], set_opts: fn(&mut DeltaOptions)) -> String {
        let dir = TempDir::new().unwrap();
        let mut zstore = Zstore::open(&dir).unwrap();
        set_opts(&mut zstore.delta_opts);

        let mut id = *EMPTY_ID20;
        for content in contents {
            let next_id = zstore.insert(content.as_ref(), &[id]).unwrap();
            id = next_id;
        }

        let tree = zstore.debug_delta_tree().unwrap();
        format!("\n{:?}", tree)
    }

    #[test]
    fn test_delta_options() {
        let noise = generate_noise(4000);
        // Similar contents. This ensures that delta will be used.
        let contents: Vec<Vec<u8>> = (0..50)
            .map(|i| format!("{}{}{}", noise, i, noise).as_bytes().to_vec())
            .collect();

        // Test that the delta trees effectively limit the max
        // delta chain length.
        //
        // Set n (max_subchain_len) and d (max_depth) to 3. Check:
        // - The chain length is bounded to 9 (n * d).
        // - 39 (see dostring of DeltaOptions) deltas are used before full text
        //   ("Depth" is 1).
        assert_eq!(
            show_tree(&contents[..42], |opts| {
                opts.max_depth = 3;
                opts.max_subchain_len = 3;
            }),
            // Hint: In case this test is broken by zstd version change, run
            // `fbcode/experimental/quark/grep-rs/cargo-test-i.py --lib` from
            // the `src` directory to auto-update the test.
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       2 |     2 |          0 |    23 |   d176e3
       3 |     3 |          0 |    23 |    480cf2
       4 |     3 |          1 |    23 |     7b2274
       5 |     3 |          2 |    23 |      9195fa
       3 |     2 |          1 |    23 |    68a44f
       4 |     3 |          0 |    23 |     049af1
       5 |     3 |          1 |    23 |      6be3a2
       6 |     3 |          2 |    23 |       0565d7
       4 |     2 |          2 |    23 |     fcf8f6
       5 |     3 |          0 |    25 |      0a012d
       6 |     3 |          1 |    24 |       316785
       7 |     3 |          2 |    24 |        7c562e
       2 |     1 |          1 |    25 |   5b8328
       3 |     2 |          0 |    23 |    8802f4
       4 |     3 |          0 |    24 |     35b10a
       5 |     3 |          1 |    24 |      565d3f
       6 |     3 |          2 |    24 |       c2a84a
       4 |     2 |          1 |    24 |     aad27f
       5 |     3 |          0 |    24 |      4ad6aa
       6 |     3 |          1 |    25 |       dd14d5
       7 |     3 |          2 |    25 |        aea69c
       5 |     2 |          2 |    25 |      0c4075
       6 |     3 |          0 |    25 |       ffa7b4
       7 |     3 |          1 |    25 |        3cfff1
       8 |     3 |          2 |    25 |         0c98c3
       3 |     1 |          2 |    25 |    091e0d
       4 |     2 |          0 |    25 |     98715e
       5 |     3 |          0 |    25 |      47258d
       6 |     3 |          1 |    25 |       7802ae
       7 |     3 |          2 |    25 |        cec4d0
       5 |     2 |          1 |    25 |      fa80d4
       6 |     3 |          0 |    25 |       1ca3c5
       7 |     3 |          1 |    25 |        5292f3
       8 |     3 |          2 |    25 |         a2ff59
       6 |     2 |          2 |    25 |       758c14
       7 |     3 |          0 |    25 |        465a4a
       8 |     3 |          1 |    25 |         3af16a
       9 |     3 |          2 |    25 |          002504
       1 |     1 |          0 |  2073 |  409bca
       2 |     2 |          0 |    25 |   cca7b8
       3 |     3 |          0 |    25 |    6972af
"#
        );

        // Test that if n = d = 0, full text will always be used.
        assert_eq!(
            show_tree(&contents[..5], |opts| {
                opts.max_depth = 0;
                opts.max_subchain_len = 0;
            }),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       1 |     1 |          0 |  2072 |  d176e3
       1 |     1 |          0 |  2072 |  480cf2
       1 |     1 |          0 |  2072 |  7b2274
       1 |     1 |          0 |  2072 |  9195fa
"#
        );

        // Test that if either n or d is a large number, it's similar to a
        // linear chain.
        assert_eq!(
            show_tree(&contents[..5], |opts| {
                opts.max_depth = 1000;
                opts.max_subchain_len = 0;
            }),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       2 |     2 |          0 |    23 |   d176e3
       3 |     3 |          0 |    23 |    480cf2
       4 |     4 |          0 |    23 |     7b2274
       5 |     5 |          0 |    23 |      9195fa
"#
        );
        assert_eq!(
            show_tree(&contents[..5], |opts| {
                opts.max_depth = 0;
                opts.max_subchain_len = 1000;
            }),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       2 |     1 |          1 |    23 |   d176e3
       3 |     1 |          2 |    23 |    480cf2
       4 |     1 |          3 |    23 |     7b2274
       5 |     1 |          4 |    23 |      9195fa
"#
        );
        assert_eq!(
            show_tree(&contents[..5], |opts| {
                opts.max_depth = 1000;
                opts.max_subchain_len = 1000;
            }),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       2 |     2 |          0 |    23 |   d176e3
       3 |     3 |          0 |    23 |    480cf2
       4 |     4 |          0 |    23 |     7b2274
       5 |     5 |          0 |    23 |      9195fa
"#
        );

        // Test that `max_chain_bytes` is effective.
        assert_eq!(
            show_tree(&contents[..10], |opts| {
                opts.max_chain_bytes = 2120;
            }),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  2072 |  12a9a7
       2 |     2 |          0 |    23 |   d176e3
       3 |     3 |          0 |    23 |    480cf2
       1 |     1 |          0 |  2072 |  7b2274
       2 |     2 |          0 |    23 |   9195fa
       3 |     3 |          0 |    23 |    68a44f
       1 |     1 |          0 |  2072 |  049af1
       2 |     2 |          0 |    23 |   6be3a2
       3 |     3 |          0 |    23 |    0565d7
       1 |     1 |          0 |  2072 |  fcf8f6
"#
        );

        // Test that `max_chain_factor_log` is effective.
        assert_eq!(
            show_tree(
                &[&noise[0..100], &noise[50..150], &noise[100..200]],
                |opts| {
                    // chain bytes should < full text length * 2 (100 * 2).
                    opts.max_chain_factor_log = 1;
                }
            ),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |    81 |  ed1309
       2 |     2 |          0 |    67 |   ab2eea
       1 |     1 |          0 |    80 |  f4d55f
"#
        );
    }

    #[test]
    fn test_delta_candidates() {
        let noise = generate_noise(5000);
        let noise = noise.as_bytes();

        // Delta candidates are not used if it cannot beat full text.
        let dir = TempDir::new().unwrap();
        let mut zstore = Zstore::open(&dir).unwrap();
        let id1 = zstore.insert(&noise[0..2000], &[]).unwrap();
        let id2 = zstore.insert(&noise[2000..], &[id1]).unwrap();
        assert_eq!(
            format!("\n{:?}", zstore.debug_delta_tree().unwrap()),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  1056 |  6e06bf
       1 |     1 |          0 |  1545 |  4ccc52
"#
        );

        // For multiple delta candidates, the one that compresses better is used.
        zstore.insert(&noise[1500..3500], &[id1, id2]).unwrap();
        assert_eq!(
            format!("\n{:?}", zstore.debug_delta_tree().unwrap()),
            r#"
Chain Len| Depth |Subchain Len| Bytes | Chain ID
       0 |     0 |          0 |     0 | da39a3
       1 |     1 |          0 |  1056 |  6e06bf
       1 |     1 |          0 |  1545 |  4ccc52
       2 |     2 |          0 |   297 |   a078da
"#
        );
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::hash::Hasher as _;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::OnceLock;

use format_util::GitCommitFields;
use format_util::HgCommitFields;
use format_util::HgTime;
use format_util::git_sha1_serialize;
use minibytes::Bytes;
use minibytes::Text;
use tracing::debug;
use tracing::trace;
use twox_hash::Xxh3Hash64;
use types::Id20;
use types::PathComponentBuf;
use types::SerializationFormat;
use types::tree::FileType;
use types::tree::TreeItemFlag;
use virtual_tree::types::FileMode;
use virtual_tree::types::TreeId;
use virtual_tree::types::TypedContentId;
use virtual_tree::types::VirtualTreeProvider;

use crate::file_size_gen::generate_file_size;
use crate::id_fields::IdFields;
use crate::id_fields::ObjectKind;
use crate::text_gen;

/// Provides read-only access to synthetic commits, trees and files. The SHA1
/// keys are synthetic and cannot pass the real checksum check.
#[derive(Clone)]
pub struct VirtualRepoProvider {
    pub(crate) format: SerializationFormat,
}

/// Provide faked trees based on [`VirtualTreeProvider`]. Also provide faked
/// blobs and commits.
impl VirtualRepoProvider {
    pub fn new(format: SerializationFormat) -> Self {
        Self { format }
    }

    /// Get the blob by (faked) SHA1 hash, without the SHA1 prefixes (git object
    /// type & size, or hg p1 p2).
    pub fn get_content(&self, id: Id20) -> Option<Bytes> {
        let fields = IdFields::maybe_from_id20(id)?;
        let len = fields.id8 >> u16::BITS;
        match fields.kind {
            crate::id_fields::ObjectKind::Blob => {
                let paragraph_seed = fields.id8 as u16;
                let text = text_gen::generate_paragraphs(len as _, paragraph_seed);
                Some(text.into_bytes().into())
            }
            crate::id_fields::ObjectKind::SymlinkBlob => {
                // Do not generate long content for symlink names.
                // OS might have length limit.
                Some(len.to_string().into_bytes().into())
            }
            crate::id_fields::ObjectKind::Tree => self.calculate_tree_bytes(fields),
            crate::id_fields::ObjectKind::Commit => self.calculate_commit_bytes(fields),
        }
    }

    /// Get the blob by (faked) SHA1 hash, with the SHA1 prefixes (git object
    /// type & size, or faked hg p1 p2).
    pub fn get_sha1_blob(&self, id: Id20) -> Option<Bytes> {
        let blob = self.get_content(id)?;
        let fields = IdFields::maybe_from_id20(id)?;
        let id8 = fields.id8;
        match self.format {
            SerializationFormat::Hg => {
                let p2 = *Id20::null_id();
                let p1 = match fields.kind {
                    ObjectKind::Commit if id8 > 0 => {
                        Id20::from(fields.with_kind_id8(ObjectKind::Commit, id8 - 1))
                    }
                    _ => p2,
                };
                let mut result = Vec::with_capacity(blob.len() + Id20::len() * 2);
                result.extend_from_slice(p2.as_ref());
                result.extend_from_slice(p1.as_ref());
                result.extend_from_slice(blob.as_ref());
                Some(result.into())
            }
            SerializationFormat::Git => {
                let kind = match fields.kind {
                    ObjectKind::Blob | ObjectKind::SymlinkBlob => "blob",
                    ObjectKind::Tree => "tree",
                    ObjectKind::Commit => "commit",
                };
                Some(git_sha1_serialize(&blob, kind).into())
            }
        }
    }

    fn calculate_tree_bytes(&self, fields: IdFields) -> Option<Bytes> {
        let tree_provider = get_tree_provider(fields.factor_bits);
        let tree_id = TreeId(NonZeroU64::new(fields.id8)?);
        let seed = tree_provider.get_tree_seed(tree_id);
        debug!(tree_id = tree_id.0, seed = seed.0, "calculating tree");
        let entries: Vec<(PathComponentBuf, Id20, TreeItemFlag)> = tree_provider
            .read_tree(tree_id)
            .map(|(name_id, content_id)| {
                let name = text_gen::generate_file_name(name_id.0.get() as _, seed.0);
                let (id20, flag) = match TypedContentId::from(content_id) {
                    TypedContentId::Tree(tree_id) => {
                        let new_id = fields.with_kind_id8(ObjectKind::Tree, tree_id.0.get());
                        trace!(name = &name, sub_tree_id = tree_id.0, "  sub-tree");
                        (Id20::from(new_id), TreeItemFlag::Directory)
                    }
                    TypedContentId::File(blob_id, file_mode) => {
                        let (kind, file_type) = match file_mode {
                            FileMode::Symlink => (ObjectKind::SymlinkBlob, FileType::Symlink),
                            FileMode::Regular => (ObjectKind::Blob, FileType::Regular),
                            FileMode::Executable => (ObjectKind::Blob, FileType::Executable),
                        };
                        let file_len =
                            calculate_file_length(seed.0, name_id.0.get(), blob_id.0.get());
                        let paragraph_seed = fold_u64_to_u16(seed.0 ^ name_id.0.get());
                        // This id8 decides file length and paragraph seed for regular blobs.
                        let new_id8 = (file_len << u16::BITS) | (paragraph_seed as u64);
                        let new_id = fields.with_kind_id8(kind, new_id8);
                        // Add salt to make different paths use different Id20s, to make it more
                        // interesting for the (potential) caching layer.
                        let id20 =
                            new_id.into_id20_with_salt(seed.0.wrapping_shl(32) ^ name_id.0.get());
                        trace!(
                            name = &name,
                            len = file_len,
                            blob_id = blob_id.0,
                            tree_seed = seed.0,
                            text_seed = paragraph_seed,
                            file_id = new_id8,
                            "  sub-file"
                        );
                        (id20, TreeItemFlag::File(file_type))
                    }
                    TypedContentId::Absent => unreachable!(),
                };
                (PathComponentBuf::from_string(name).unwrap(), id20, flag)
            })
            .collect();
        // PERF: Maybe basic_serialize_tree can take a "stream" to avoid allocation?
        storemodel::basic_serialize_tree(entries, self.format).ok()
    }

    fn calculate_commit_bytes(&self, fields: IdFields) -> Option<Bytes> {
        let id8 = fields.id8;
        let tree_id20 = {
            let tree_provider = get_tree_provider(fields.factor_bits);
            if id8 >= tree_provider.root_tree_len() as u64 {
                return None;
            }
            let root_tree_id = tree_provider.root_tree_id(id8 as _);
            Id20::from(fields.with_kind_id8(ObjectKind::Tree, root_tree_id.0.get()))
        };

        const AUTHOR: Text = Text::from_static("test <test@example.com>");
        // NOTE: Commit message is short for now. To exercise the commit message
        // storage, one might want to use a longer message with a larger entropy.
        let message = Text::from(format!("synthetic commit {}", id8 + 1));
        // Python's datetime.MAXYEAR = 9999
        // Practically, get_tree_provider(factor_bits=23) hits this.
        const MAX_UNIXTIME: i64 = 253402070400;
        let date = {
            const START_UNIXTIME: u64 = 1761263091;
            HgTime {
                unixtime: ((START_UNIXTIME + id8) as i64).min(MAX_UNIXTIME),
                offset: 0,
            }
        };

        let text = match self.format {
            SerializationFormat::Hg => {
                let fields = HgCommitFields {
                    tree: tree_id20,
                    author: AUTHOR,
                    date,
                    message,
                    ..HgCommitFields::default()
                };
                fields.to_text().ok()?
            }
            SerializationFormat::Git => {
                let mut parents = Vec::new();
                if id8 > 0 {
                    parents.push(Id20::from(
                        fields.with_kind_id8(ObjectKind::Commit, id8 - 1),
                    ));
                }
                let fields = GitCommitFields {
                    tree: tree_id20,
                    parents,
                    author: AUTHOR,
                    date,
                    committer: AUTHOR,
                    committer_date: date,
                    message,
                    ..GitCommitFields::default()
                };
                fields.to_text().ok()?
            }
        };

        let bytes = Bytes::from(text.into_bytes());
        Some(bytes)
    }
}

fn calculate_file_length(seed: u64, name_id: u64, blob_id: u64) -> u64 {
    // Use xxhash to get a (roughly) uniform distribution.
    let x = {
        let mut hash = Xxh3Hash64::default();
        hash.write(&seed.to_le_bytes());
        hash.write(&name_id.to_le_bytes());
        hash.write(&(blob_id >> 8).to_le_bytes());
        hash.finish()
    };
    let len = generate_file_size(x);
    // blob_id is meant to be the "generation number" of the file.
    // But it can also be very large! So let's just truncate it.
    len + ((blob_id & 0xff) << 5)
}

fn fold_u64_to_u16(x: u64) -> u16 {
    let b64 = x.to_le_bytes();
    let b16: [u8; 2] = [
        b64[0] ^ b64[2] ^ b64[4] ^ b64[6],
        b64[1] ^ b64[3] ^ b64[5] ^ b64[7],
    ];
    u16::from_le_bytes(b16)
}

/// The virtual tree uses u64 (64 bits) internally for various operations.
/// To avoid overflow, limit the factor_bits to 34.
/// The default virtual repo with factor_bits=34 has about 200+ trillion files,
/// which should probably be good enough.
pub const MAX_FACTOR_BITS: usize = 34;

pub(crate) fn get_tree_provider(factor_bits: u8) -> &'static Arc<dyn VirtualTreeProvider> {
    let factor_bits = factor_bits as usize;
    assert!(factor_bits <= MAX_FACTOR_BITS);
    static TREE_PROVIDER_PER_FACTOR_BITS: LazyLock<
        [OnceLock<Arc<dyn VirtualTreeProvider>>; MAX_FACTOR_BITS + 1],
    > = LazyLock::new(|| std::array::from_fn(|_| OnceLock::new()));
    TREE_PROVIDER_PER_FACTOR_BITS[factor_bits].get_or_init(|| {
        let tree_provider = Arc::new(virtual_tree::serialized::EXAMPLE1.clone());
        virtual_tree::stretch::stretch_trees(tree_provider, factor_bits as _)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fold_u64_to_u16() {
        assert_eq!(fold_u64_to_u16(0x1234), 0x1234);
        assert_eq!(fold_u64_to_u16(0x10300204), 0x1234);
    }
}

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
        match fields.kind {
            crate::id_fields::ObjectKind::Blob => {
                Some(text_gen::generate_file_content_of_length(fields.id8 as _))
            }
            crate::id_fields::ObjectKind::SymlinkBlob => {
                // Do not generate long content for symlink names.
                // OS might have length limit.
                Some(fields.id8.to_string().into_bytes().into())
            }
            crate::id_fields::ObjectKind::Tree => self.calculate_tree_bytes(fields),
            crate::id_fields::ObjectKind::Commit => self.calcualte_commit_bytes(fields),
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
        let entries: Vec<(PathComponentBuf, Id20, TreeItemFlag)> = tree_provider
            .read_tree(tree_id)
            .map(|(name_id, content_id)| {
                let name = text_gen::generate_file_name(name_id.0.get() as _, seed.0);
                let (id20, flag) = match TypedContentId::from(content_id) {
                    TypedContentId::Tree(tree_id) => {
                        let new_id = fields.with_kind_id8(ObjectKind::Tree, tree_id.0.get());
                        (Id20::from(new_id), TreeItemFlag::Directory)
                    }
                    TypedContentId::File(blob_id, file_mode) => {
                        let (kind, file_type) = match file_mode {
                            FileMode::Symlink => (ObjectKind::SymlinkBlob, FileType::Symlink),
                            FileMode::Regular => (ObjectKind::Blob, FileType::Regular),
                            FileMode::Executable => (ObjectKind::Blob, FileType::Executable),
                        };
                        // This id8 decides file length for regular blobs.
                        let id8 = calculate_file_length(seed.0, name_id.0.get(), blob_id.0.get());
                        let new_id = fields.with_kind_id8(kind, id8);
                        (Id20::from(new_id), TreeItemFlag::File(file_type))
                    }
                    TypedContentId::Absent => unreachable!(),
                };
                (PathComponentBuf::from_string(name).unwrap(), id20, flag)
            })
            .collect();
        // PERF: Maybe basic_serialize_tree can take a "stream" to avoid allocation?
        storemodel::basic_serialize_tree(entries, self.format).ok()
    }

    fn calcualte_commit_bytes(&self, fields: IdFields) -> Option<Bytes> {
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
        let date = {
            const START_UNIXTIME: u64 = 1761263091;
            HgTime {
                unixtime: (START_UNIXTIME + id8) as i64,
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
        hash.finish()
    };
    let len = generate_file_size(x);
    // blob_id is the "generation number" of the file. It can make the file a bit longer.
    len + (blob_id << 5)
}

/// The virtual tree uses u64 (64 bits) internally for various operations.
/// To avoid overflow, limit the factor_bits to 34.
/// The default virtual repo with factor_bits=34 has about 200+ trillion files,
/// which should probably be good enough.
const MAX_FACTOR_BITS: usize = 34;

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

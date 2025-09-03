/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::fs::read_to_string;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use blob::Blob;
use dag::Dag;
use dag::Group;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::ops::DagAddHeads;
use dag::ops::DagPersistent;
use format_util::commit_text_to_root_tree_id;
use format_util::git_sha1_deserialize;
use format_util::git_sha1_serialize;
use format_util::hg_sha1_deserialize;
use format_util::hg_sha1_serialize;
use format_util::split_hg_file_metadata;
use futures::lock::Mutex;
use futures::lock::MutexGuard;
use manifest_augmented_tree::AugmentedDirectoryNode;
use manifest_augmented_tree::AugmentedFileNode;
use manifest_augmented_tree::AugmentedTree;
use manifest_augmented_tree::AugmentedTreeEntry;
use manifest_augmented_tree::AugmentedTreeWithDigest;
use manifest_tree::FileType;
use manifest_tree::Flag;
use manifest_tree::PathComponentBuf;
use manifest_tree::TreeEntry;
use manifest_tree::TreeManifest;
use metalog::CommitOptions;
use metalog::MetaLog;
use minibytes::Bytes;
use mutationstore::MutationStore;
use parking_lot::RawRwLock;
use parking_lot::RwLock;
use parking_lot::lock_api::RwLockReadGuard;
use repourl::RepoUrl;
use sha1::Digest;
use sha1::Sha1;
use storemodel::FileAuxData;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::types::CasDigest;
use storemodel::types::HgId;
use storemodel::types::Parents;
use storemodel::types::hgid::NULL_ID;
use tracing::instrument;
use zstore::Id20;
use zstore::Zstore;

use crate::Result;

const HG_PARENTS_LEN: usize = HgId::len() * 2;
const HG_LEN: usize = HgId::len();

/// Non-lazy, pure Rust, local repo implementation.
///
/// Mainly useful as a simple "server repo" in tests that can replace ssh remote
/// repos and exercise SaplingRemoteApi features.
///
/// Format-wise, an eager repo includes:
///
/// ## SHA1 Key/Value Content Store
///
/// See [`EagerRepoStore`].
///
/// ## Commit Graph
///
/// Commit hashes and parent commit hashes.
///
/// Currently backed by the [`dag::Dag`]. It handles the main complexity.
///
///
/// ## Metadata
///
/// Bookmarks, tip, remote bookmarks, visible heads, etc.
///
/// Format is made compatible with the Python code. Only bookmarks is
/// implemented for now to support testing use-cases.
///
/// Currently backed by [`metalog::MetaLog`]. It's a lightweight source control
/// for atomic metadata changes.
pub struct EagerRepo {
    pub(crate) dag: Mutex<Dag>,
    pub(crate) store: EagerRepoStore,
    metalog: RwLock<MetaLog>,
    pub(crate) dir: PathBuf,
    pub(crate) mut_store: Mutex<MutationStore>,
}

/// Storage used by `EagerRepo`. Wrapped by `Arc<RwLock>` for easier sharing.
///
/// File, tree, commit contents.
///
/// SHA1 is verifiable. For HG this means `sorted([p1, p2])` and filelog rename
/// metadata is included in values. For Git this means `type size` is part of
/// the prefix of the stored blobs.
///
/// This is meant to be mainly a content store. We currently "abuse" it to
/// answer filelog history when ght HG format is used. The filelog (filenode)
/// and linknodes are considered tech-debt and we hope to replace them with
/// fastlog APIs which serve sub-graph with `(commit, path)` as graph nodes.
///
/// Unlike file history, we don't use `(p1, p2)` for commit parents because it
/// loses the parent order, which is important for commits. The callsite should
/// use a dedicated DAG implementation to answer commit parents questions.
///
/// Currently backed by [`zstore::Zstore`], a pure content key-value store.
#[derive(Clone)]
pub struct EagerRepoStore {
    pub(crate) inner: Arc<RwLock<Zstore>>,
    pub(crate) format: SerializationFormat,
}

impl EagerRepoStore {
    /// Open an [`EagerRepoStore`] at the given directory.
    /// Create an empty store on demand.
    pub fn open(dir: &Path, format: SerializationFormat) -> Result<Self> {
        let inner = Zstore::open(dir)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
            format,
        })
    }

    /// Flush changes to disk.
    pub fn flush(&self) -> Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    /// In git's case, the `data` should include the type and size header.
    pub fn add_sha1_blob(&self, data: &[u8], bases: &[Id20]) -> Result<Id20> {
        let mut inner = self.inner.write();
        Ok(inner.insert(data, bases)?)
    }

    /// Insert arbitrary blob with an `id`.
    /// This is usually used for hg's LFS data.
    pub fn add_arbitrary_blob(&self, id: Id20, data: &[u8]) -> Result<()> {
        let mut inner = self.inner.write();
        inner.insert_arbitrary(id, data, &[])?;
        Ok(())
    }

    /// Read SHA1 blob from zstore, including the prefixes.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        let inner = self.inner.read();
        Ok(inner.get(id)?)
    }

    pub(crate) fn format(&self) -> SerializationFormat {
        self.format
    }

    /// Read the blob with its p1, p2 prefix removed.
    pub fn get_content(&self, id: Id20) -> Result<Option<Bytes>> {
        // Special null case.
        if id.is_null() {
            return Ok(Some(Bytes::default()));
        }
        match self.get_sha1_blob(id)? {
            None => Ok(None),
            Some(data) => {
                let content = match self.format() {
                    SerializationFormat::Hg => hg_sha1_deserialize(&data)?.0,
                    SerializationFormat::Git => git_sha1_deserialize(&data)?.0,
                };
                Ok(Some(data.slice_to_bytes(content)))
            }
        }
    }

    /// Read CAS data for digest.
    #[tracing::instrument(skip(self), level = "trace")]
    pub fn get_cas_blob(&self, digest: CasDigest) -> Result<Option<Bytes>> {
        ::fail::fail_point!("eagerepo::cas", |_| {
            Err(anyhow!("stub eagerepo CAS error").into())
        });

        let Some(pointer_data) = self.get_sha1_blob(digest_id(digest))? else {
            tracing::trace!("no CAS pointer data");
            return Ok(None);
        };

        let pointer = CasPointer::deserialize(&pointer_data)?;
        tracing::trace!("found CAS pointer {pointer:?}");

        match CasPointer::deserialize(&pointer_data)? {
            CasPointer::Tree(id) => {
                // We store data for AugmentedTreeWithDigest, but we want to return data for AugmentedTree.
                // Strip off the first line (which is the digest).
                self.get_sha1_blob(augmented_id(id)).and_then(|blob| {
                    tracing::trace!("found CAS tree data");
                    blob.map(|blob| {
                        if let Some(idx) = blob.as_ref().iter().position(|&b| b == b'\n') {
                            Ok(blob.slice(idx + 1..))
                        } else {
                            Err(anyhow!("augmented tree data has no newline?").into())
                        }
                    })
                    .transpose()
                })
            }
            CasPointer::File(id) => match self.get_content(id)? {
                Some(data) => {
                    tracing::trace!("found CAS file data");
                    Ok(Some(split_hg_file_metadata(&data).0))
                }
                None => Ok(None),
            },
        }
    }

    /// Read SHA1 blob from zstore for augmented data.
    pub fn get_augmented_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        self.get_sha1_blob(augmented_id(id))
    }

    /// Check files and trees referenced by the `id` are present.
    /// Missing paths are pushed to `missing`.
    fn find_missing_references(
        &self,
        id: Id20,
        flag: Flag,
        path: PathInfo,
        missing: &mut Vec<PathInfo>,
    ) -> Result<()> {
        // Cannot check submodule reference.
        if matches!(flag, Flag::File(FileType::GitSubmodule)) {
            return Ok(());
        }
        // Check file or tree reference.
        let content = match self.get_content(id)? {
            Some(content) => content,
            None => {
                missing.push(path);
                return Ok(());
            }
        };
        // Check subfiles or subtrees.
        if matches!(flag, Flag::Directory) {
            let entry = TreeEntry(content, self.format());
            for element in entry.elements() {
                let element = element?;
                let name = element.component.into_string();
                let path = path.join(name);
                self.find_missing_references(element.hgid, element.flag, path, missing)?;
            }
        }
        Ok(())
    }
}

/// Used by `check_tree`, `check_file` to report missing path.
#[derive(Clone)]
struct PathInfo(Arc<PathInfoInner>);
struct PathInfoInner {
    name: String,
    parent: Option<PathInfo>,
}

impl fmt::Display for PathInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let inner = &self.0;
        if let Some(parent) = inner.parent.as_ref() {
            parent.fmt(f)?;
        }
        if !inner.name.is_empty() {
            write!(f, "/{}", inner.name)?;
        }
        Ok(())
    }
}

impl PathInfo {
    fn root() -> Self {
        let inner = PathInfoInner {
            name: String::new(),
            parent: None,
        };
        Self(Arc::new(inner))
    }

    fn join(&self, name: String) -> Self {
        let inner = PathInfoInner {
            name,
            parent: Some(self.clone()),
        };
        Self(Arc::new(inner))
    }
}

impl EagerRepo {
    /// Open an [`EagerRepo`] at the given directory. Create an empty repo on demand.
    pub fn open(dir: &Path) -> Result<Self> {
        // Auto-detect Git format from path. "*-git" or "*.git" use the Git format.
        // Resolve to full path since "." might be a "-git" path.
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let format = match dir.file_name().and_then(|s| s.to_str()) {
            Some(s) if s.ends_with(".git") || s.ends_with("-git") => SerializationFormat::Git,
            _ => SerializationFormat::Hg,
        };
        tracing::trace!(?dir, ?format, "EagerRepo::open");
        let ident = identity::sniff_dir(&dir)?.unwrap_or_else(identity::default);
        // Attempt to match directory layout of a real client repo.
        let hg_dir = dir.join(ident.dot_dir());
        let store_dir = hg_dir.join("store");
        let dag = Dag::open(store_dir.join("segments").join("v1"))?;
        let store = EagerRepoStore::open(&store_dir.join("hgcommits").join("v1"), format)?;
        let metalog = MetaLog::open(store_dir.join("metalog"), None)?;
        let mut_store = MutationStore::open(store_dir.join("mutation"))?;

        let repo = Self {
            dag: Mutex::new(dag),
            store,
            metalog: RwLock::new(metalog),
            dir: dir.to_path_buf(),
            mut_store: Mutex::new(mut_store),
        };

        // "eagercompat" is a revlog repo secretly using an eager store under the hood.
        // It's requirements don't match our expectations, so return early. This is mainly
        // so we can access the EagerRepo SaplingRemoteApi trait implementation.
        if has_eagercompat_requirement(&store_dir) {
            return Ok(repo);
        }

        // Write "requires" files.
        write_requires(&hg_dir, &["store", "treestate", "windowssymlinks"])?;
        let mut store_requires = vec![
            "narrowheads",
            "visibleheads",
            "segmentedchangelog",
            "eagerepo",
            "invalidatelinkrev",
        ];
        if matches!(format, SerializationFormat::Git) {
            store_requires.push("git");
        }
        write_requires(&store_dir, &store_requires)?;
        Ok(repo)
    }

    /// Convert an URL to a directory path that can be passed to `open`.
    ///
    /// Supported URLs:
    /// - `eager:dir_path`, `eager://dir_path`
    /// - `test:name`, `test://name`: same as `eager:$TESTTMP/name`
    /// - `/path/to/dir` where the path is a EagerRepo.
    #[instrument(level = "trace", ret)]
    pub fn url_to_dir(url: &RepoUrl) -> Option<PathBuf> {
        if url.scheme() == "eager" {
            return Some(url.path().to_string().into());
        }

        if url.scheme() == "test" {
            let path = url.path();
            let path = path.trim_start_matches('/');
            if let Some(tmp) = testtmp() {
                let path = tmp.join(path);
                return Some(path);
            }
        }

        if let Some(mut path) = url.resolved_str().strip_prefix("ssh://user@dummy/") {
            // Strip off any query params.
            if let Some(query_start) = path.find('?') {
                path = &path[..query_start];
            }

            // Allow instantiating EagerRepo for dummyssh servers. This is so we can get a
            // working SaplingRemoteApi for server repos in legacy tests.
            if let Some(tmp) = testtmp() {
                let path = Path::new(&tmp).join(path);
                if let Ok(Some(ident)) = identity::sniff_dir(&path) {
                    let mut store_path = path.clone();
                    store_path.push(ident.dot_dir());
                    store_path.push("store");
                    if has_eagercompat_requirement(&store_path) || is_eager_repo(&path) {
                        return Some(path);
                    } else {
                        tracing::trace!("no eagercompat requirement for dummy URL");
                    }
                } else {
                    tracing::trace!("no identity for dummy URL");
                }
            }
        }

        if url.scheme() == "file" {
            let path = PathBuf::from(url.path());
            if is_eager_repo(&path) {
                return Some(path.to_path_buf());
            } else {
                tracing::trace!("file URL isn't an eagerepo (no eagerepo requirement)");
            }
        }

        None
    }

    /// Write pending changes to disk.
    pub async fn flush(&self) -> Result<()> {
        self.store.flush()?;
        let master_heads = {
            let books = self.get_bookmarks_map()?;
            let mut heads = Vec::new();
            for name in ["master", "main"] {
                if let Some(id) = books.get(name) {
                    heads.push(Vertex::copy_from(id.as_ref()));
                    break;
                }
            }
            VertexListWithOptions::from(heads).with_desired_group(Group::MASTER)
        };
        self.dag.lock().await.flush(&master_heads).await?;
        let opts = CommitOptions::default();
        self.metalog.write().commit(opts)?;
        self.mut_store.lock().await.flush().await?;
        Ok(())
    }

    // The following APIs provide low-level ways to read or write the repo.
    //
    // They are used for push before SaplingRemoteApi provides push related APIs.

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    pub fn add_sha1_blob(&self, data: &[u8]) -> Result<Id20> {
        // SPACE: This does not utilize zstore's delta features to save space.
        self.store.add_sha1_blob(data, &[])
    }

    /// Read SHA1 blob from zstore.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        self.store.get_sha1_blob(id)
    }

    /// Insert SHA1 blob to zstore for augmented trees.
    /// These blobs are not content addressed
    pub fn add_augmented_tree_blob(&self, id: Id20, digest: CasDigest, data: &[u8]) -> Result<()> {
        self.store.add_arbitrary_blob(augmented_id(id), data)?;
        // Store a mapping from CasDigest to hg id so we can query augmented data by CasDigest.
        self.add_cas_mapping(digest, CasPointer::Tree(id))
    }

    fn add_cas_mapping(&self, digest: CasDigest, pointer: CasPointer) -> Result<()> {
        tracing::trace!("adding CAS mapping from {digest:?} to {pointer:?}");
        self.store
            .add_arbitrary_blob(digest_id(digest), &pointer.serialize())
    }

    pub(crate) fn format(&self) -> SerializationFormat {
        self.store.format()
    }

    /// Extract parents out of a SHA1 manifest blob, returns the remaining data.
    /// The callsite needs to ensure the store format is hg.
    fn extract_parents_from_tree_data_hg(data: Bytes) -> Result<(Parents, Bytes)> {
        let p2 = HgId::from_slice(&data[..HG_LEN]).map_err(anyhow::Error::from)?;
        let p1 = HgId::from_slice(&data[HG_LEN..HG_PARENTS_LEN]).map_err(anyhow::Error::from)?;
        Ok((Parents::new(p1, p2), data.slice(HG_PARENTS_LEN..)))
    }

    /// Parse a file blob into raw data and copy_from metadata.
    /// The callsite needs to ensure the store format is hg.
    fn parse_file_blob_hg(data: Bytes) -> (Bytes, Bytes) {
        // drop the p1/p2 info
        let data = data.slice(HG_PARENTS_LEN..);
        let (raw_data, copy_from) = format_util::split_hg_file_metadata(&data);
        (raw_data, copy_from)
    }

    /// Calculate augmented trees recursively
    pub fn derive_augmented_tree_recursively(&self, id: Id20) -> Result<Option<Bytes>> {
        if !matches!(self.format(), SerializationFormat::Hg) {
            return Err(crate::Error::Other(anyhow!(
                "Augmented tree is only supported for Hg format"
            )));
        }
        match self.store.get_augmented_blob(id)? {
            Some(t) => Ok(Some(t)),
            None => {
                let sapling_manifest = self.get_sha1_blob(id)?;
                if sapling_manifest.is_none() {
                    // Can't really calculate because corresponding sapling manifest is missing
                    return Ok(None);
                }
                let sapling_manifest = sapling_manifest.unwrap();
                let (parents, data) = Self::extract_parents_from_tree_data_hg(sapling_manifest)?;
                let tree_entry = manifest_tree::TreeEntry(data, SerializationFormat::Hg);
                let mut entries: Vec<(PathComponentBuf, AugmentedTreeEntry)> = Vec::new();
                let mut sapling_tree_blob_size = 0;
                for child in tree_entry.elements() {
                    let child = child?;
                    let hgid = child.hgid;
                    let entry: AugmentedTreeEntry = match child.flag {
                        Flag::Directory => {
                            let subtree_bytes = self.derive_augmented_tree_recursively(hgid)?;
                            if subtree_bytes.is_none() {
                                return Ok(None); // Can't calculate because subtree's data is missing.
                            }
                            let CasDigest { hash, size } =
                                AugmentedTreeWithDigest::try_deserialize_digest(
                                    &mut std::io::Cursor::new(subtree_bytes.unwrap()),
                                )?;

                            sapling_tree_blob_size += HgId::hex_len() + 1;

                            AugmentedTreeEntry::DirectoryNode(AugmentedDirectoryNode {
                                treenode: hgid,
                                augmented_manifest_id: hash,
                                augmented_manifest_size: size,
                            })
                        }
                        Flag::File(file_type) => {
                            let bytes = self.get_sha1_blob(hgid)?;
                            if bytes.is_none() {
                                return Ok(None); // Can't calculate because file is missing.
                            }
                            let (raw_data, copy_from) = Self::parse_file_blob_hg(bytes.unwrap());
                            let aux_data = FileAuxData::from_content(&Blob::Bytes(raw_data));

                            // Store a mapping from CasDigest to hg id so we can query augmented data by CasDigest.
                            self.add_cas_mapping(
                                CasDigest {
                                    hash: aux_data.blake3,
                                    size: aux_data.total_size,
                                },
                                CasPointer::File(hgid),
                            )?;

                            sapling_tree_blob_size += HgId::hex_len();

                            AugmentedTreeEntry::FileNode(AugmentedFileNode {
                                file_type,
                                filenode: hgid,
                                content_blake3: aux_data.blake3,
                                content_sha1: aux_data.sha1,
                                total_size: aux_data.total_size,
                                file_header_metadata: if copy_from.is_empty() {
                                    None
                                } else {
                                    Some(copy_from)
                                },
                            })
                        }
                    };
                    sapling_tree_blob_size += child.component.len() + 2;
                    entries.push((child.component, entry));
                }

                let aug_tree = AugmentedTree {
                    hg_node_id: id,
                    computed_hg_node_id: None,
                    p1: parents.p1().copied(),
                    p2: parents.p2().copied(),
                    entries,
                    sapling_tree_blob_size,
                };

                let digest = aug_tree.compute_content_addressed_digest()?;

                let aug_tree_with_digest = AugmentedTreeWithDigest {
                    augmented_manifest_id: digest.hash,
                    augmented_manifest_size: digest.size,
                    augmented_tree: aug_tree,
                };

                let mut buf: Vec<u8> =
                    Vec::with_capacity(aug_tree_with_digest.serialized_tree_blob_size());
                aug_tree_with_digest
                    .try_serialize(&mut buf)
                    .expect("writing failed");

                // Store the augmented tree in zstore
                self.add_augmented_tree_blob(id, digest, &buf)?;

                Ok(Some(Bytes::from(buf)))
            }
        }
    }

    /// Insert a commit. Return the commit hash.
    pub async fn add_commit(&self, parents: &[Id20], raw_text: &[u8]) -> Result<Id20> {
        let id: Id20 = {
            let data = match self.format() {
                SerializationFormat::Git => git_sha1_serialize(raw_text, "commit"),
                SerializationFormat::Hg => {
                    let p1 = parents.first().cloned();
                    let p2 = parents.get(1).cloned();
                    hg_sha1_serialize(raw_text, &p1.unwrap_or(NULL_ID), &p2.unwrap_or(NULL_ID))
                }
            };
            self.add_sha1_blob(&data)?
        };

        let vertex: Vertex = { Vertex::copy_from(id.as_ref()) };
        let parents: Vec<Vertex> = parents
            .iter()
            .map(|v| Vertex::copy_from(v.as_ref()))
            .collect();

        // Check paths referred by the commit are present.
        //
        // PERF: This is sub-optimal for large working copies.
        // Ideally we check per tree insertion and only checks
        // the root tree without recursion. But that requires
        // new APIs to insert trees, and insert trees in a
        // certain order.
        if let Ok(tree_id) = commit_text_to_root_tree_id(raw_text, self.format()) {
            let mut missing = Vec::new();
            let path = PathInfo::root();
            self.store
                .find_missing_references(tree_id, Flag::Directory, path, &mut missing)?;
            if !missing.is_empty() {
                let paths = missing.into_iter().map(|p| p.to_string()).collect();
                return Err(crate::Error::CommitMissingPaths(
                    vertex,
                    Vertex::copy_from(tree_id.as_ref()),
                    paths,
                ));
            }
        }

        let parent_map: HashMap<Vertex, Vec<Vertex>> =
            vec![(vertex.clone(), parents)].into_iter().collect();
        self.dag
            .lock()
            .await
            .add_heads(&parent_map, &vec![vertex].into())
            .await?;

        Ok(id)
    }

    /// Update or remove a single bookmark.
    pub fn set_bookmark(&self, name: &str, id: Option<Id20>) -> Result<()> {
        let mut bookmarks = self.get_bookmarks_map()?;
        match id {
            None => bookmarks.remove(name),
            Some(id) => bookmarks.insert(name.to_string(), id),
        };
        self.set_bookmarks_map(bookmarks)?;
        Ok(())
    }

    /// Get the commit id of a bookmark.
    pub fn get_bookmark(&self, name: &str) -> Result<Option<Id20>> {
        let bookmarks = self.get_bookmarks_map()?;
        let id = bookmarks.get(name).cloned();
        Ok(id)
    }

    /// Get bookmarks.
    pub fn get_bookmarks_map(&self) -> Result<BTreeMap<String, Id20>> {
        // Attempt to match the format used by a real client repo.
        let text: String = {
            let data = self.metalog.read().get("bookmarks")?;
            let opt_text = data.map(|b| String::from_utf8_lossy(&b).to_string());
            opt_text.unwrap_or_default()
        };
        let map = text
            .lines()
            .filter_map(|line| {
                // example line: d59acbf094f61c10b72dff3d0e6085b5c75d14f4 foo
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.len() == 2 {
                    if let Ok(id) = Id20::from_hex(words[0].as_bytes()) {
                        return Some((words[1].to_string(), id));
                    }
                }
                None
            })
            .collect();
        Ok(map)
    }

    /// Set bookmarks.
    pub fn set_bookmarks_map(&self, map: BTreeMap<String, Id20>) -> Result<()> {
        for (name, id) in map.iter() {
            if self.store.get_content(*id)?.is_none() {
                return Err(crate::Error::BookmarkMissingCommit(
                    name.to_string(),
                    Vertex::copy_from(id.as_ref()),
                ));
            }
        }
        let text = map
            .into_iter()
            .map(|(name, id)| format!("{} {}\n", id.to_hex(), name))
            .collect::<Vec<_>>()
            .concat();
        self.metalog.write().set("bookmarks", text.as_bytes())?;
        Ok(())
    }

    /// Get the tree manifest of a commit.
    pub async fn commit_to_manifest(&self, commit_id: HgId) -> Result<TreeManifest> {
        let commit_to_root_tree = self.store.read_root_tree_ids(vec![commit_id]).await?;
        if commit_to_root_tree.is_empty() {
            return Err(anyhow!("commit {} cannot be found", commit_id.to_hex()).into());
        }
        let (_, tree_id) = commit_to_root_tree[0];
        Ok(TreeManifest::durable(Arc::new(self.store.clone()), tree_id))
    }

    /// Obtain a reference to the commit graph.
    pub async fn dag(&self) -> MutexGuard<'_, Dag> {
        self.dag.lock().await
    }

    /// Obtain a reference to the metalog.
    pub fn metalog(&self) -> RwLockReadGuard<'_, RawRwLock, MetaLog> {
        self.metalog.read()
    }

    /// Obtain an instance to the store.
    pub fn store(&self) -> EagerRepoStore {
        self.store.clone()
    }
}

fn testtmp() -> Option<PathBuf> {
    std::env::var("TESTTMP").ok().map(|tmp| {
        let mut tmp = PathBuf::from(tmp);
        // Look for ".testtmp" bread crumb. If we are running from EdenFS, our $TESTTMP
        // env var might not be up to date.
        if let Ok(bread_crumb) = read_to_string(tmp.join(".testtmp")) {
            tmp = PathBuf::from(bread_crumb);
        }
        tmp
    })
}

// Hash something else in to differentiate augmented and non-augmented keys.
fn augmented_id(id: Id20) -> Id20 {
    let mut hasher = Sha1::new();
    hasher.update(b"augmented");
    hasher.update(id.as_ref());
    let hash: [u8; 20] = hasher.finalize().into();
    Id20::from_byte_array(hash)
}

fn digest_id(digest: CasDigest) -> Id20 {
    let mut hasher = Sha1::new();
    hasher.update(digest.hash.as_ref());
    let hash: [u8; 20] = hasher.finalize().into();
    Id20::from_byte_array(hash)
}

// Point to either a tree blob or file blob. Tree blobs are stored in the desired
// augmented format, but files are stored with hg metadata prepended, so we must
// differentiat the two.
#[derive(Debug)]
enum CasPointer {
    Tree(Id20),
    File(Id20),
}

impl CasPointer {
    fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(21);
        match self {
            Self::File(id) => {
                data.push(0);
                data.extend_from_slice(id.as_ref());
            }
            Self::Tree(id) => {
                data.push(1);
                data.extend_from_slice(id.as_ref());
            }
        }
        data
    }

    fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() != 21 {
            Err(anyhow!("bad CAS pointer length {}", data.len()).into())
        } else {
            match data[0] {
                0 => Ok(Self::File(Id20::from_slice(&data[1..]).unwrap())),
                1 => Ok(Self::Tree(Id20::from_slice(&data[1..]).unwrap())),
                _ => Err(anyhow!("bad pointer type {}", data[0]).into()),
            }
        }
    }
}

pub fn is_eager_repo(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_dir() {
        return false;
    }

    if let Ok(Some(ident)) = identity::sniff_dir(path) {
        // Check store requirements
        let store_requirement_path = path.join(ident.dot_dir()).join("store").join("requires");
        if let Ok(s) = std::fs::read_to_string(store_requirement_path) {
            if s.lines().any(|s| s == "eagerepo") {
                return true;
            }
        }
    }

    false
}

fn has_eagercompat_requirement(store_path: &Path) -> bool {
    std::fs::read_to_string(store_path.join("requires"))
        .is_ok_and(|r| r.split('\n').any(|r| r == "eagercompat"))
}

/// Write "requires" in the given directory, if it does not exist already.
/// If "requires" exists and does not match the given content, raise an error.
fn write_requires(dir: &Path, requires: &[&'static str]) -> Result<()> {
    let path = dir.join("requires");
    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(mut f) => {
            let mut requires: String = requires.join("\n");
            requires.push('\n');
            f.write_all(requires.as_bytes())?;
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let actual: BTreeSet<String> = fs::read_to_string(&path)?
                .lines()
                .map(|l| l.to_string())
                .collect();
            let expected: BTreeSet<String> = requires.iter().map(|r| r.to_string()).collect();
            let unsupported: Vec<String> = actual.difference(&expected).cloned().collect();
            let missing: Vec<String> = expected.difference(&actual).cloned().collect();
            if unsupported.is_empty() && missing.is_empty() {
                Ok(())
            } else {
                Err(crate::Error::RequirementsMismatch(
                    path.display().to_string(),
                    unsupported,
                    missing,
                ))
            }
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use manifest_tree::PathComponentBuf;
    use manifest_tree::TreeElement;

    use super::*;

    #[tokio::test]
    async fn test_read_write_blob() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        let text = &b"blob-text-foo-bar"[..];
        let id = repo.add_sha1_blob(text).unwrap();
        assert_eq!(repo.get_sha1_blob(id).unwrap().as_deref(), Some(text));

        // Pending changes are invisible until flush.
        let repo2 = EagerRepo::open(dir).unwrap();
        assert!(repo2.get_sha1_blob(id).unwrap().is_none());

        repo.flush().await.unwrap();

        let repo2 = EagerRepo::open(dir).unwrap();
        assert_eq!(repo2.get_sha1_blob(id).unwrap().as_deref(), Some(text));
    }

    #[tokio::test]
    async fn test_add_commit() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        let commit1 = repo.add_commit(&[], b"A").await.unwrap();
        let commit2 = repo.add_commit(&[], b"B").await.unwrap();
        let _commit3 = repo.add_commit(&[commit1, commit2], b"C").await.unwrap();
        repo.flush().await.unwrap();

        let repo2 = EagerRepo::open(dir).unwrap();
        let rendered = dag::render::render_dag(&*repo2.dag().await, |v| {
            let id = Id20::from_slice(v.as_ref()).unwrap();
            let blob = repo2.get_sha1_blob(id).unwrap().unwrap();
            Some(String::from_utf8_lossy(&blob[Id20::len() * 2..]).to_string())
        })
        .unwrap();
        assert_eq!(
            rendered,
            r#"
            o    53cceda7b244d25793af31655d682c7fe67d7650 C
            ├─╮
            │ o  35e7525ce3a48913275d7061dd9a867ffef1e34d B
            │
            o  005d992c5dcf32993668f7cede29d296c494a5d9 A"#
        );
    }

    #[tokio::test]
    async fn test_read_write_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        let commit1 = repo.add_commit(&[], b"A").await.unwrap();
        let commit2 = repo.add_commit(&[], b"B").await.unwrap();
        repo.set_bookmark("c1", Some(commit1)).unwrap();
        repo.set_bookmark("stable", Some(commit1)).unwrap();
        repo.set_bookmark("main", Some(commit2)).unwrap();
        repo.flush().await.unwrap();

        let repo = EagerRepo::open(dir).unwrap();
        assert_eq!(
            format!("{:#?}", repo.get_bookmarks_map().unwrap()),
            r#"{
    "c1": HgId("005d992c5dcf32993668f7cede29d296c494a5d9"),
    "main": HgId("35e7525ce3a48913275d7061dd9a867ffef1e34d"),
    "stable": HgId("005d992c5dcf32993668f7cede29d296c494a5d9"),
}"#
        );
        repo.set_bookmark("c1", None).unwrap();
        repo.set_bookmark("stable", Some(commit2)).unwrap();
        assert_eq!(
            format!("{:#?}", repo.get_bookmarks_map().unwrap()),
            r#"{
    "main": HgId("35e7525ce3a48913275d7061dd9a867ffef1e34d"),
    "stable": HgId("35e7525ce3a48913275d7061dd9a867ffef1e34d"),
}"#
        );
    }

    #[test]
    fn test_requires_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        drop(repo);

        let ident = identity::sniff_dir(dir)
            .unwrap()
            .unwrap_or_else(identity::default);
        fs::write(
            dir.join(ident.dot_dir()).join("requires"),
            "store\nremotefilelog\n",
        )
        .unwrap();

        let err = EagerRepo::open(dir).map(|_| ()).unwrap_err();
        match err {
            crate::Error::RequirementsMismatch(_, unsupported, missing) => {
                assert_eq!(unsupported, ["remotefilelog"]);
                assert_eq!(missing, ["treestate", "windowssymlinks"]);
            }
            _ => panic!("expect RequirementsMismatch, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_add_commit_find_missing_referencess() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        let missing_id = missing_id();

        // Root tree missing.
        let err = repo
            .add_commit(&[], missing_id.to_hex().as_bytes())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "when adding commit e9644aa7950f61cfe12d510c623692698fee0e4c with root tree 35e7525ce3a48913275d7061dd9a867ffef1e34d, referenced paths [\"\"] are not present"
        );

        // Subdir or subfile missing.
        let p =
            |s: &str| -> PathComponentBuf { PathComponentBuf::from_string(s.to_string()).unwrap() };
        let subtree_content = TreeEntry::from_elements(
            vec![
                TreeElement::new(p("a"), missing_id, Flag::Directory),
                TreeElement::new(p("b"), missing_id, Flag::File(FileType::Regular)),
            ],
            SerializationFormat::Hg,
        )
        .to_bytes();
        let subtree_id = repo
            .add_sha1_blob(&hg_sha1_serialize(
                &subtree_content,
                Id20::null_id(),
                Id20::null_id(),
            ))
            .unwrap();
        let root_tree_content = TreeEntry::from_elements(
            vec![
                TreeElement::new(p("c"), subtree_id, Flag::Directory),
                TreeElement::new(p("d"), missing_id, Flag::File(FileType::Regular)),
            ],
            SerializationFormat::Hg,
        )
        .to_bytes();
        let root_tree_id = repo
            .add_sha1_blob(&hg_sha1_serialize(
                &root_tree_content,
                Id20::null_id(),
                Id20::null_id(),
            ))
            .unwrap();
        let err = repo
            .add_commit(&[], root_tree_id.to_hex().as_bytes())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "when adding commit 6870320ce60748a99108dd1be33b52b58b277baa with root tree 5a725b18a26fd10416fd93c5bd26fa0265ac2579, referenced paths [\"/c/a\", \"/c/b\", \"/d\"] are not present"
        );
    }

    #[test]
    fn test_set_bookmark_missing_commit() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let repo = EagerRepo::open(dir).unwrap();
        let missing_id = missing_id();

        let err = repo.set_bookmark("a", Some(missing_id)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "when moving bookmark \"a\" to 35e7525ce3a48913275d7061dd9a867ffef1e34d, the commit does not exist"
        );
    }

    fn missing_id() -> Id20 {
        Id20::from_hex(b"35e7525ce3a48913275d7061dd9a867ffef1e34d").unwrap()
    }
}

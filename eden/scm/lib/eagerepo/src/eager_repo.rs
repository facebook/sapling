/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use dag::ops::DagAddHeads;
use dag::ops::DagPersistent;
use dag::Dag;
use dag::Group;
use dag::Vertex;
use dag::VertexListWithOptions;
use manifest_tree::FileType;
use manifest_tree::Flag;
use manifest_tree::TreeEntry;
use metalog::CommitOptions;
use metalog::MetaLog;
use minibytes::Bytes;
use parking_lot::RwLock;
use storemodel::TreeFormat;
use zstore::Id20;
use zstore::Zstore;

use crate::Result;

/// Non-lazy, pure Rust, local repo implementation.
///
/// Mainly useful as a simple "server repo" in tests that can replace ssh remote
/// repos and exercise EdenApi features.
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
    pub(crate) dag: Dag,
    store: EagerRepoStore,
    metalog: MetaLog,
    pub(crate) dir: PathBuf,
}

/// Storage used by `EagerRepo`. Wrapped by `Arc<RwLock>` for easier sharing.
///
/// File, tree, commit contents.
///
/// SHA1 is verifiable. For HG this means `sorted([p1, p2])` and filelog rename
/// metadata is included in values.
///
/// This is meant to be mainly a content store. We currently "abuse" it to
/// answer filelog history. The filelog (filenode) and linknodes are
/// considered tech-debt and we hope to replace them with fastlog APIs which
/// serve sub-graph with `(commit, path)` as graph nodes.
///
/// We don't use `(p1, p2)` for commit parents because it loses the parent
/// order. The DAG storage is used to answer commit parents instead.
///
/// Currently backed by [`zstore::Zstore`]. For simplicity, we don't use the
/// zstore delta-compress features, and don't store different types separately.
#[derive(Clone)]
pub struct EagerRepoStore {
    inner: Arc<RwLock<Zstore>>,
}

impl EagerRepoStore {
    /// Open an [`EagerRepoStore`] at the given directory.
    /// Create an empty store on demand.
    pub fn open(dir: &Path) -> Result<Self> {
        let inner = Zstore::open(dir)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
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
    pub fn add_sha1_blob(&self, data: &[u8], bases: &[Id20]) -> Result<Id20> {
        let mut inner = self.inner.write();
        Ok(inner.insert(data, bases)?)
    }

    /// Read SHA1 blob from zstore.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        let inner = self.inner.read();
        Ok(inner.get(id)?)
    }

    /// Read the blob with its p1, p2 prefix removed.
    pub fn get_content(&self, id: Id20) -> Result<Option<Bytes>> {
        // Prefix in bytes of the hg SHA1s in the eagerepo data.
        const HG_SHA1_PREFIX: usize = Id20::len() * 2;
        match self.get_sha1_blob(id)? {
            None => Ok(None),
            Some(data) => Ok(Some(data.slice(HG_SHA1_PREFIX..))),
        }
    }

    /// Check files and trees referenced by the `id` are present.
    /// Missing paths are pushed to `missing`.
    fn find_missing_references<'a>(
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
            let entry = TreeEntry(content, TreeFormat::Hg);
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
        let ident = identity::sniff_dir(dir)?.unwrap_or_else(identity::default);
        // Attempt to match directory layout of a real client repo.
        let hg_dir = dir.join(ident.dot_dir());
        let store_dir = hg_dir.join("store");
        let dag = Dag::open(store_dir.join("segments").join("v1"))?;
        let store = EagerRepoStore::open(&store_dir.join("hgcommits").join("v1"))?;
        let metalog = MetaLog::open(store_dir.join("metalog"), None)?;
        // Write "requires" files.
        write_requires(&hg_dir, &["store", "treestate"])?;
        write_requires(
            &store_dir,
            &[
                "narrowheads",
                "visibleheads",
                "segmentedchangelog",
                "eagerepo",
                "invalidatelinkrev",
            ],
        )?;
        let repo = Self {
            dag,
            store,
            metalog,
            dir: dir.to_path_buf(),
        };
        Ok(repo)
    }

    /// Convert an URL to a directory path that can be passed to `open`.
    ///
    /// Supported URLs:
    /// - `eager:dir_path`, `eager://dir_path`
    /// - `test:name`, `test://name`: same as `eager:$TESTTMP/server-repos/name`
    pub fn url_to_dir(value: &str) -> Option<PathBuf> {
        let prefix = "eager:";
        if let Some(path) = value.strip_prefix(prefix) {
            let path: PathBuf = if cfg!(windows) {
                // Remove '//' prefix from Windows file path. This makes it
                // possible to use paths like 'eager://C:\foo\bar'.
                let path = path.trim_start_matches('/');
                // Replace '/' with '\' on Windows so one can write code like
                // eager://$TESTTMP/foo/bar in test. This is important if
                // $TESTTMP is a UNC path, since / won't work with a UNC path.
                let path = path.replace('/', "\\");
                Path::new(&path).to_path_buf()
            } else {
                Path::new(path).to_path_buf()
            };
            return Some(path);
        }
        let prefix = "test:";
        if let Some(path) = value.strip_prefix(prefix) {
            let path = path.trim_start_matches('/');
            if let Ok(tmp) = std::env::var("TESTTMP") {
                let tmp: &Path = Path::new(&tmp);
                let path = tmp.join(path);
                return Some(path);
            }
        }
        None
    }

    /// Write pending changes to disk.
    pub async fn flush(&mut self) -> Result<()> {
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
            VertexListWithOptions::from(heads).with_highest_group(Group::MASTER)
        };
        self.dag.flush(&master_heads).await?;
        let opts = CommitOptions::default();
        self.metalog.commit(opts)?;
        Ok(())
    }

    // The following APIs provide low-level ways to read or write the repo.
    //
    // They are used for push before EdenApi provides push related APIs.

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    pub fn add_sha1_blob(&mut self, data: &[u8]) -> Result<Id20> {
        // SPACE: This does not utilize zstore's delta features to save space.
        self.store.add_sha1_blob(data, &[])
    }

    /// Read SHA1 blob from zstore.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        self.store.get_sha1_blob(id)
    }

    /// Insert a commit. Return the commit hash.
    pub async fn add_commit(&mut self, parents: &[Id20], raw_text: &[u8]) -> Result<Id20> {
        let parents: Vec<Vertex> = parents
            .iter()
            .map(|v| Vertex::copy_from(v.as_ref()))
            .collect();
        let id: Id20 = {
            let data = hg_sha1_text(&parents, raw_text);
            self.add_sha1_blob(&data)?
        };
        let vertex: Vertex = { Vertex::copy_from(id.as_ref()) };

        // Check paths referred by the commit are present.
        //
        // PERF: This is sub-optimal for large working copies.
        // Ideally we check per tree insertion and only checks
        // the root tree without recursion. But that requires
        // new APIs to insert trees, and insert trees in a
        // certain order.
        if let Some(hex_tree_id) = raw_text.get(0..Id20::hex_len()) {
            if let Ok(tree_id) = Id20::from_hex(hex_tree_id) {
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
        }

        let parent_map: HashMap<Vertex, Vec<Vertex>> =
            vec![(vertex.clone(), parents)].into_iter().collect();
        self.dag
            .add_heads(&parent_map, &vec![vertex].into())
            .await?;
        Ok(id)
    }

    /// Update or remove a single bookmark.
    pub fn set_bookmark(&mut self, name: &str, id: Option<Id20>) -> Result<()> {
        let mut bookmarks = self.get_bookmarks_map()?;
        match id {
            None => bookmarks.remove(name),
            Some(id) => bookmarks.insert(name.to_string(), id),
        };
        self.set_bookmarks_map(bookmarks)?;
        Ok(())
    }

    /// Get bookmarks.
    pub fn get_bookmarks_map(&self) -> Result<BTreeMap<String, Id20>> {
        // Attempt to match the format used by a real client repo.
        let text: String = {
            let data = self.metalog.get("bookmarks")?;
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
    pub fn set_bookmarks_map(&mut self, map: BTreeMap<String, Id20>) -> Result<()> {
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
        self.metalog.set("bookmarks", text.as_bytes())?;
        Ok(())
    }

    /// Obtain a reference to the commit graph.
    pub fn dag(&self) -> &Dag {
        &self.dag
    }

    /// Obtain a reference to the metalog.
    pub fn metalog(&self) -> &MetaLog {
        &self.metalog
    }

    /// Obtain an instance to the store.
    pub fn store(&self) -> EagerRepoStore {
        self.store.clone()
    }
}

/// Convert parents and raw_text to HG SHA1 text format.
fn hg_sha1_text(parents: &[Vertex], raw_text: &[u8]) -> Vec<u8> {
    fn null_id() -> Vertex {
        Vertex::copy_from(Id20::null_id().as_ref())
    }
    let mut result = Vec::with_capacity(raw_text.len() + Id20::len() * 2);
    let (p1, p2) = (
        parents.get(0).cloned().unwrap_or_else(null_id),
        parents.get(1).cloned().unwrap_or_else(null_id),
    );
    if p1 < p2 {
        result.extend_from_slice(p1.as_ref());
        result.extend_from_slice(p2.as_ref());
    } else {
        result.extend_from_slice(p2.as_ref());
        result.extend_from_slice(p1.as_ref());
    }
    result.extend_from_slice(&raw_text);
    result
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

        let mut repo = EagerRepo::open(dir).unwrap();
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

        let mut repo = EagerRepo::open(dir).unwrap();
        let commit1 = repo.add_commit(&[], b"A").await.unwrap();
        let commit2 = repo.add_commit(&[], b"B").await.unwrap();
        let _commit3 = repo.add_commit(&[commit1, commit2], b"C").await.unwrap();
        repo.flush().await.unwrap();

        let repo2 = EagerRepo::open(dir).unwrap();
        let rendered = dag::render::render_namedag(repo2.dag(), |v| {
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

        let mut repo = EagerRepo::open(dir).unwrap();
        let commit1 = repo.add_commit(&[], b"A").await.unwrap();
        let commit2 = repo.add_commit(&[], b"B").await.unwrap();
        repo.set_bookmark("c1", Some(commit1)).unwrap();
        repo.set_bookmark("stable", Some(commit1)).unwrap();
        repo.set_bookmark("main", Some(commit2)).unwrap();
        repo.flush().await.unwrap();

        let mut repo = EagerRepo::open(dir).unwrap();
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
                assert_eq!(missing, ["treestate"]);
            }
            _ => panic!("expect RequirementsMismatch, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_add_commit_find_missing_referencess() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let mut repo = EagerRepo::open(dir).unwrap();
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
            TreeFormat::Hg,
        )
        .to_bytes();
        let subtree_id = repo
            .add_sha1_blob(&hg_sha1_text(&[], &subtree_content))
            .unwrap();
        let root_tree_content = TreeEntry::from_elements(
            vec![
                TreeElement::new(p("c"), subtree_id, Flag::Directory),
                TreeElement::new(p("d"), missing_id, Flag::File(FileType::Regular)),
            ],
            TreeFormat::Hg,
        )
        .to_bytes();
        let root_tree_id = repo
            .add_sha1_blob(&hg_sha1_text(&[], &root_tree_content))
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

        let mut repo = EagerRepo::open(dir).unwrap();
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

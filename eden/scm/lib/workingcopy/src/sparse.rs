/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::DerefMut;
use std::path::Path;
use std::sync::Arc;

use anyhow::Error;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use pathmatcher::ExactMatcher;
use pathmatcher::UnionMatcher;
pub use sparse::Root;
use storemodel::FileStore;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::VFS;

pub static CONFIG_OVERRIDE_CACHE: &str = "sparseprofileconfigs";
pub static MERGE_FILE_OVERRIDES: &str = "tempsparse";

pub fn repo_matcher(
    vfs: &VFS,
    dot_path: &Path,
    manifest: &impl Manifest,
    store: Arc<dyn FileStore>,
) -> anyhow::Result<Option<(DynMatcher, u64)>> {
    repo_matcher_with_overrides(vfs, dot_path, manifest, store, &disk_overrides(dot_path)?)
}

pub fn repo_matcher_with_overrides(
    vfs: &VFS,
    dot_path: &Path,
    manifest: &impl Manifest,
    store: Arc<dyn FileStore>,
    overrides: &HashMap<String, String>,
) -> anyhow::Result<Option<(DynMatcher, u64)>> {
    let prof = match fs_err::read(dot_path.join("sparse")) {
        Ok(contents) => sparse::Root::from_bytes(contents, ".hg/sparse".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    let (matcher, mut hasher) = build_matcher(&prof, manifest, store.clone(), overrides)?;

    let mut matcher: DynMatcher = Arc::new(matcher);

    match fs_err::read_to_string(dot_path.join(MERGE_FILE_OVERRIDES)) {
        Ok(temp) => {
            temp.hash(&mut hasher);
            let exact = ExactMatcher::new(
                temp.split('\n')
                    .map(|p| p.try_into())
                    .collect::<Result<Vec<&RepoPath>, _>>()?
                    .iter(),
                vfs.case_sensitive(),
            );
            matcher = Arc::new(UnionMatcher::new(vec![Arc::new(exact), matcher]));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    Ok(Some((matcher, hasher.finish())))
}

#[tracing::instrument(skip_all)]
pub fn build_matcher(
    prof: &sparse::Root,
    manifest: &impl Manifest,
    store: Arc<dyn FileStore>,
    overrides: &HashMap<String, String>,
) -> anyhow::Result<(sparse::Matcher, DefaultHasher)> {
    let hasher = Mutex::new(DefaultHasher::new());
    prof.hash(hasher.lock().deref_mut());

    let matcher = prof.matcher(|path| {
        let file_id = {
            let repo_path = RepoPathBuf::from_string(path.clone())?;

            // This might block.
            match manifest.get(&repo_path)? {
                None => {
                    tracing::warn!(?repo_path, "non-existent sparse profile include");
                    Ok::<_, Error>(None)
                }
                Some(fs_node) => match fs_node {
                    FsNodeMetadata::File(FileMetadata { hgid, .. }) => Ok(Some(hgid)),
                    FsNodeMetadata::Directory(_) => {
                        tracing::warn!(?repo_path, "sparse profile include is a directory");
                        Ok(None)
                    }
                },
            }
        };

        let file_id = match file_id? {
            Some(id) => id,
            None => return Ok(None),
        };

        let repo_path = RepoPathBuf::from_string(path.clone())?;
        let bytes = store.get_content(&repo_path, file_id)?;
        let mut bytes = bytes.into_vec();
        if let Some(extra) = overrides.get(&path) {
            bytes.append(&mut extra.to_string().into_bytes());
        }
        bytes.hash(hasher.lock().deref_mut());

        tracing::debug!(path, size = bytes.len(), "fetched included profile");

        Ok(Some(bytes))
    })?;

    Ok((matcher, hasher.into_inner()))
}

pub fn config_overrides(config: impl Config) -> HashMap<String, String> {
    let mut overrides: HashMap<String, String> = HashMap::new();
    for key in config.keys("sparseprofile") {
        let parts: Vec<&str> = key.splitn(3, '.').collect();
        if parts.len() != 3 {
            tracing::warn!(?key, "invalid sparseprofile config key");
            continue;
        }

        let (sparse_section, prof_name) = (parts[0], parts[2]);

        let vals = match config.get_or_default::<Vec<String>>("sparseprofile", &key) {
            Ok(vals) => vals,
            Err(err) => {
                tracing::warn!(?key, ?err, "invalid sparseprofile config value");
                continue;
            }
        };

        overrides
            .entry(prof_name.into())
            .or_default()
            .push_str(&format!(
                "\n# source = hgrc.dynamic \"{}\"\n[{}]\n{}\n# source =\n",
                key,
                sparse_section,
                vals.join("\n")
            ));
    }

    overrides
}

pub fn disk_overrides(dot_path: &Path) -> anyhow::Result<HashMap<String, String>> {
    // Pick up cached overrides written out by sparse.py during checkout.
    // The ".<pid>" file contains uncommited overrides for an in-progress checkout.
    for loc in [
        format!("{}.{}", CONFIG_OVERRIDE_CACHE, std::process::id()),
        CONFIG_OVERRIDE_CACHE.to_string(),
    ] {
        match util::file::open(dot_path.join(&loc), "r") {
            Ok(f) => return Ok(serde_json::from_reader(f)?),
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => continue,
        }
    }

    Ok(HashMap::new())
}

pub fn sparse_matcher(
    vfs: &VFS,
    manifests: &[Arc<TreeManifest>],
    store: Arc<dyn FileStore>,
    dot_dir: &Path,
) -> anyhow::Result<Option<DynMatcher>> {
    assert!(!manifests.is_empty());

    let mut sparse_matchers: Vec<DynMatcher> = Vec::new();
    for manifest in manifests.iter() {
        if let Some((matcher, _hash)) = repo_matcher(
            vfs,
            &vfs.root().join(dot_dir),
            manifest.as_ref(),
            store.clone(),
        )? {
            sparse_matchers.push(matcher);
        }
    }

    if sparse_matchers.is_empty() {
        // Indicates we have no .hg/sparse (i.e. sparse is disabled).
        Ok(None)
    } else {
        Ok(Some(Arc::new(UnionMatcher::new(sparse_matchers))))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pathmatcher::Matcher;
    use storemodel::minibytes::Bytes;
    use storemodel::KeyStore;
    use types::HgId;
    use types::Parents;
    use types::RepoPath;

    use super::*;

    #[test]
    fn test_config_overrides() -> anyhow::Result<()> {
        let mut conf = BTreeMap::new();

        conf.insert("sparseprofile.include.foo.someprof", "inca,incb");
        conf.insert("sparseprofile.include.bar.someprof", "incc");
        conf.insert("sparseprofile.exclude.foo.someprof", "exca,excb");

        conf.insert("sparseprofile.include.foo.otherprof", "inca");

        let overrides = config_overrides(&conf);
        assert_eq!(
            overrides,
            HashMap::from([
                (
                    "someprof".to_string(),
                    r#"
# source = hgrc.dynamic "exclude.foo.someprof"
[exclude]
exca
excb
# source =

# source = hgrc.dynamic "include.bar.someprof"
[include]
incc
# source =

# source = hgrc.dynamic "include.foo.someprof"
[include]
inca
incb
# source =
"#
                    .to_string()
                ),
                (
                    "otherprof".to_string(),
                    r#"
# source = hgrc.dynamic "include.foo.otherprof"
[include]
inca
# source =
"#
                    .to_string()
                ),
            ])
        );

        let dir = tempfile::tempdir()?;

        {
            let f = util::file::create(dir.path().join(CONFIG_OVERRIDE_CACHE))?;
            serde_json::to_writer(f, &overrides)?;
        }

        let roundtrip_overrides = disk_overrides(dir.path())?;
        assert_eq!(roundtrip_overrides, overrides);

        Ok(())
    }

    #[test]
    fn test_build_matcher() -> anyhow::Result<()> {
        let root_dir = tempfile::tempdir()?;
        let vfs = VFS::new(root_dir.path().to_path_buf())?;

        let mut config = BTreeMap::new();

        config.insert("sparseprofile.exclude.blah.tools/sparse/base", "inc/exc");

        let mut commit = StubCommit::new();
        commit.insert(
            "tools/sparse/base",
            "[include]
inc

# ignore this non-existent profile
%include bogus-ignore-me

# ignore include of directory
%include tools/sparse

[exclude]
exc",
        );

        let (matcher, _hash) = build_matcher(
            &sparse::Root::from_bytes(b"%include tools/sparse/base", "root".to_string())?,
            &commit,
            Arc::new(commit.clone()),
            &config_overrides(&config),
        )?;

        assert!(matcher.matches_file("inc/banana".try_into()?)?);
        assert!(!matcher.matches_file("exc/banana".try_into()?)?);
        assert!(!matcher.matches_file("merge/a".try_into()?)?);

        // Test the config override.
        assert!(!matcher.matches_file("inc/exc/foo".try_into()?)?);

        fs_err::write(
            root_dir.path().join(MERGE_FILE_OVERRIDES),
            "merge/a\nmerge/b",
        )?;

        fs_err::write(root_dir.path().join("sparse"), "%include tools/sparse/base")?;

        let (matcher, _hash) =
            repo_matcher(&vfs, root_dir.path(), &commit, Arc::new(commit.clone()))?.unwrap();

        assert!(matcher.matches_file("merge/a".try_into()?)?);
        assert!(matcher.matches_file("merge/b".try_into()?)?);
        assert!(!matcher.matches_file("merge/abc".try_into()?)?);

        Ok(())
    }

    #[test]
    fn test_matcher_hashes() -> anyhow::Result<()> {
        let config: BTreeMap<String, String> = BTreeMap::new();

        let mut commit = StubCommit::new();
        commit.insert(
            "tools/sparse/base",
            "[include]
inc

[exclude]
exc",
        );

        let (_matcher, hash) = build_matcher(
            &sparse::Root::from_bytes(b"%include tools/sparse/base", "root".to_string())?,
            &commit,
            Arc::new(commit.clone()),
            &config_overrides(&config),
        )?;

        let mut commit = StubCommit::new();
        commit.insert(
            "tools/sparse/base",
            "[include]
inc

[exclude]
exc",
        );

        let (_matcher, same_hash) = build_matcher(
            &sparse::Root::from_bytes(b"%include tools/sparse/base", "root".to_string())?,
            &commit,
            Arc::new(commit.clone()),
            &config_overrides(&config),
        )?;

        assert!(
            hash.finish() == same_hash.finish(),
            "hashes should match if contents matches"
        );

        let (_matcher, different_hash_config_change) = build_matcher(
            &sparse::Root::from_bytes(
                b"%include tools/sparse/base
[include]
config_inc
",
                "root".to_string(),
            )?,
            &commit,
            Arc::new(commit.clone()),
            &config_overrides(&config),
        )?;

        assert_ne!(
            hash.finish(),
            different_hash_config_change.finish(),
            "hashes should not match if contents do not match"
        );

        let mut commit = StubCommit::new();
        commit.insert(
            "tools/sparse/base",
            "[include]
inc
",
        );

        let (_matcher, different_hash_profile_change) = build_matcher(
            &sparse::Root::from_bytes(b"%include tools/sparse/base", "root".to_string())?,
            &commit,
            Arc::new(commit.clone()),
            &config_overrides(&config),
        )?;

        assert_ne!(
            hash.finish(),
            different_hash_profile_change.finish(),
            "hashes should not match if contents do not match"
        );

        Ok(())
    }

    #[derive(Clone)]
    struct StubCommit {
        files: HashMap<RepoPathBuf, Vec<u8>>,
    }

    impl StubCommit {
        fn new() -> Self {
            StubCommit {
                files: HashMap::new(),
            }
        }

        fn insert(&mut self, path: impl AsRef<str>, contents: impl AsRef<[u8]>) {
            self.files.insert(
                path.as_ref().to_string().try_into().unwrap(),
                contents.as_ref().to_vec(),
            );
        }

        fn file_id(&self, path: &RepoPath) -> Option<HgId> {
            self.files
                .get(path)
                .map(|data| HgId::from_content(data, Parents::None))
        }
    }

    #[allow(unused_variables)]
    impl Manifest for StubCommit {
        fn get(&self, path: &RepoPath) -> anyhow::Result<Option<FsNodeMetadata>> {
            match self.file_id(path) {
                None => Ok(None),
                Some(id) => Ok(Some(FsNodeMetadata::File(FileMetadata::new(
                    id,
                    manifest::FileType::Regular,
                )))),
            }
        }

        fn get_ignore_case(&self, path: &RepoPath) -> anyhow::Result<Option<FsNodeMetadata>> {
            unimplemented!("get_ignore_case not implemented for StubCommit")
        }

        fn list(&self, path: &RepoPath) -> anyhow::Result<manifest::List> {
            unimplemented!()
        }

        fn insert(
            &mut self,
            file_path: RepoPathBuf,
            file_metadata: FileMetadata,
        ) -> anyhow::Result<()> {
            unimplemented!()
        }

        fn remove(&mut self, file_path: &RepoPath) -> anyhow::Result<Option<FileMetadata>> {
            unimplemented!()
        }

        fn flush(&mut self) -> anyhow::Result<HgId> {
            unimplemented!()
        }

        fn files<'a, M: 'static + pathmatcher::Matcher + Sync + Send>(
            &'a self,
            matcher: M,
        ) -> Box<dyn Iterator<Item = anyhow::Result<manifest::File>> + 'a> {
            unimplemented!()
        }

        fn dirs<'a, M: 'static + pathmatcher::Matcher + Sync + Send>(
            &'a self,
            matcher: M,
        ) -> Box<dyn Iterator<Item = anyhow::Result<manifest::Directory>> + 'a> {
            unimplemented!()
        }

        fn diff<'a, M: pathmatcher::Matcher>(
            &'a self,
            other: &'a Self,
            matcher: &'a M,
        ) -> anyhow::Result<Box<dyn Iterator<Item = anyhow::Result<manifest::DiffEntry>> + 'a>>
        {
            unimplemented!()
        }

        fn modified_dirs<'a, M: pathmatcher::Matcher>(
            &'a self,
            other: &'a Self,
            matcher: &'a M,
        ) -> anyhow::Result<Box<dyn Iterator<Item = anyhow::Result<manifest::DirDiffEntry>> + 'a>>
        {
            unimplemented!()
        }
    }

    #[async_trait::async_trait]
    impl KeyStore for StubCommit {
        fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Bytes>> {
            match self.file_id(path) {
                Some(id) if id == hgid => {
                    Ok(Some(Bytes::copy_from_slice(self.files.get(path).unwrap())))
                }
                _ => Ok(None),
            }
        }
    }

    #[async_trait::async_trait]
    impl FileStore for StubCommit {}
}

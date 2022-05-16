/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use async_runtime::try_block_unless_interrupted;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use storemodel::futures::StreamExt;
use storemodel::ReadFileContents;
use types::Key;
use types::RepoPathBuf;

static CONFIG_OVERRIDE_CACHE: &str = "sparseprofileconfigs";

pub fn sparse_matcher(
    config: impl Config,
    root_profile: impl AsRef<[u8]>,
    root_profile_source: String,
    manifest: impl Manifest + Send + Sync + 'static,
    store: impl ReadFileContents<Error = anyhow::Error> + Send + Sync,
    dot_hg_path: &Path,
) -> anyhow::Result<sparse::Matcher> {
    let prof = sparse::Root::from_bytes(root_profile, root_profile_source)?;
    let overrides = config_overrides(config);

    util::file::atomic_write(&dot_hg_path.join(CONFIG_OVERRIDE_CACHE), |f| {
        serde_json::to_writer(f, &overrides)?;
        Ok(())
    })?;

    let manifest = Arc::new(manifest);

    let matcher = try_block_unless_interrupted(prof.matcher(|path| async {
        let path = path;

        let file_id = {
            let manifest = manifest.clone();
            let repo_path = RepoPathBuf::from_string(path.clone())?;

            // Work around nested block_on() calls by spawning a new thread.
            // Once the Manifest is async this can go away.
            tokio::task::spawn_blocking(move || match manifest.get(&repo_path)? {
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
            })
            .await??
        };

        let file_id = match file_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let repo_path = RepoPathBuf::from_string(path.clone())?;
        let mut stream = store
            .read_file_contents(vec![Key::new(repo_path.clone(), file_id.clone())])
            .await;
        match stream.next().await {
            Some(Ok((bytes, _key))) => {
                let mut bytes = bytes.into_vec();
                if let Some(extra) = overrides.get(&path) {
                    bytes.append(&mut extra.to_string().into_bytes());
                }
                Ok(Some(bytes))
            }
            Some(Err(err)) => Err(err),
            None => Err(anyhow!("no contents for {}", repo_path)),
        }
    }))?;

    Ok(matcher)
}

fn config_overrides(config: impl Config) -> HashMap<String, String> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::stream;
    use futures::stream::BoxStream;
    use tempfile::tempdir;
    use types::HgId;
    use types::Parents;
    use types::RepoPath;

    use super::*;

    #[test]
    fn test_config_overrides() {
        let mut conf = BTreeMap::new();

        conf.insert("sparseprofile.include.foo.someprof", "inca,incb");
        conf.insert("sparseprofile.include.bar.someprof", "incc");
        conf.insert("sparseprofile.exclude.foo.someprof", "exca,excb");

        conf.insert("sparseprofile.include.foo.otherprof", "inca");

        assert_eq!(
            config_overrides(&conf),
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
    }

    #[test]
    fn test_sparse_matcher() {
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

        let td = tempdir().unwrap();

        let matcher = sparse_matcher(
            &config,
            "%include tools/sparse/base",
            "root".to_string(),
            commit.clone(),
            commit.clone(),
            td.path(),
        )
        .unwrap();

        assert_eq!(
            matcher.explain("inc/banana".try_into().unwrap()).unwrap(),
            (true, "root -> tools/sparse/base".to_string())
        );

        assert_eq!(
            matcher.explain("exc/banana".try_into().unwrap()).unwrap(),
            (false, "root -> tools/sparse/base".to_string())
        );

        // Test the config override.
        assert_eq!(
            matcher.explain("inc/exc/foo".try_into().unwrap()).unwrap(),
            (
                false,
                r#"root -> tools/sparse/base (hgrc.dynamic "exclude.blah.tools/sparse/base")"#
                    .to_string()
            )
        );

        // Make sure we wrote out the overrides cache file.
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(td.path().join(CONFIG_OVERRIDE_CACHE)).unwrap()
            )
            .unwrap(),
            serde_json::json!({
                "tools/sparse/base": "\n# source = hgrc.dynamic \"exclude.blah.tools/sparse/base\"\n[exclude]\ninc/exc\n# source =\n",
            })
        );
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
    impl ReadFileContents for StubCommit {
        type Error = anyhow::Error;

        async fn read_file_contents(
            &self,
            keys: Vec<Key>,
        ) -> BoxStream<Result<(storemodel::minibytes::Bytes, Key), Self::Error>> {
            stream::iter(keys.into_iter().map(|k| match self.file_id(&k.path) {
                None => Err(anyhow!("no such path")),
                Some(id) if id == k.hgid => {
                    Ok((self.files.get(&k.path).unwrap().clone().into(), k))
                }
                Some(_) => Err(anyhow!("bad file id")),
            }))
            .boxed()
        }
    }
}

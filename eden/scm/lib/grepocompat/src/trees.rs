/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use grepomanifest::parse::parse_manifest;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest_tree::Manifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeManifestReadNode;
use storemodel::FileStore;
use storemodel::TreeStore;
use types::FetchContext;
use types::FileType;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::path::GrepoPathTranslator;

const REPO_XML_PATH: &str = "static/static.xml";

/// Parse .repo manifest xml files and synthesizes repo projects as trees.
/// TODO: linkfile and copyfile support
pub fn synthesize_grepo_projects(
    tree_store: &Arc<dyn TreeStore>,
    file_store: &Arc<dyn FileStore>,
    manifest: &TreeManifest,
) -> Result<TreeManifest> {
    let repo_xml_path: &RepoPath = REPO_XML_PATH.try_into()?;
    let metadata = match manifest.get(repo_xml_path)? {
        Some(FsNodeMetadata::File(metadata)) => metadata,
        _ => anyhow::bail!("repo manifest xml not found at {repo_xml_path} in tree"),
    };
    let xml_data = file_store
        .get_content(FetchContext::default(), repo_xml_path, metadata.hgid)?
        .into_bytes();
    let projects = parse_manifest(&xml_data)?.projects;

    let mut new_manifest = TreeManifest::ephemeral(tree_store.clone());
    // Project paths are stored with suffix encoding so a path can be both
    // a file (GitSubmodule entry) and a directory (containing nested projects).
    // The returned manifest has a `PathTranslator` set so consumers see
    // decoded paths transparently.
    new_manifest.set_path_translator(Arc::new(GrepoPathTranslator));

    for (path, project) in projects {
        if let Some(revision) = &project.revision {
            let repo_path = RepoPathBuf::try_from(path)?;
            let hgid = HgId::from_hex(revision.as_bytes())?;
            new_manifest.insert(repo_path, FileMetadata::new(hgid, FileType::GitSubmodule))?;
        }
    }

    Ok(new_manifest)
}

/// Synthesize Grepo project manifests after prefetching their shared inputs in batches.
///
/// The outer error is a batch failure. Each inner result is the result for the manifest at the
/// same position.
pub fn synthesize_grepo_projects_batch(
    tree_store: &Arc<dyn TreeStore>,
    file_store: &Arc<dyn FileStore>,
    manifests: Vec<TreeManifest>,
) -> Result<Vec<Result<TreeManifest>>> {
    prefetch_grepo_project_inputs(file_store, &manifests)?;
    Ok(manifests
        .iter()
        .map(|manifest| synthesize_grepo_projects(tree_store, file_store, manifest))
        .collect())
}

/// Prefetch the tree levels and XML blobs needed to synthesize a manifest batch.
///
/// All manifests must use the same tree store.
fn prefetch_grepo_project_inputs(
    file_store: &Arc<dyn FileStore>,
    manifests: &[TreeManifest],
) -> Result<()> {
    if manifests.is_empty() {
        return Ok(());
    }

    let repo_xml_path: &RepoPath = REPO_XML_PATH.try_into()?;
    let components = repo_xml_path
        .components()
        .map(|component| component.to_owned())
        .collect::<Vec<_>>();
    let mut nodes = manifests
        .iter()
        .map(TreeManifest::read_root)
        .collect::<Result<Vec<_>>>()?;

    // Walk one level at a time so every manifest fetches that level in one batch.
    // Skip a manifest without the XML. `synthesize_grepo_projects` reports its error.
    for component in components {
        TreeManifestReadNode::prefetch(&nodes)?;
        nodes = nodes
            .into_iter()
            .map(|node| {
                Ok(node
                    .lookup_children(std::slice::from_ref(&component))?
                    .pop()
                    .flatten())
            })
            .filter_map(Result::transpose)
            .collect::<Result<Vec<_>>>()?;
    }

    // Many commits share one XML blob. Remove duplicate keys before the prefetch.
    let mut keys = nodes
        .into_iter()
        .filter_map(|node| match node.metadata() {
            FsNodeMetadata::File(metadata) => {
                Some(Key::new(repo_xml_path.to_owned(), metadata.hgid))
            }
            FsNodeMetadata::Directory(_) => None,
        })
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    if keys.is_empty() {
        Ok(())
    } else {
        file_store.prefetch(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use blob::Blob;
    use manifest_tree::testutil::TestStore;
    use storemodel::InsertOpts;
    use storemodel::KeyStore;
    use storemodel::Kind;
    use types::testutil::hgid;
    use types::testutil::repo_path;
    use types::testutil::repo_path_buf;

    use super::*;

    #[derive(Clone)]
    struct RecordingFileStore {
        inner: Arc<TestStore>,
        prefetched: Arc<Mutex<Vec<Vec<Key>>>>,
    }

    impl RecordingFileStore {
        fn new(inner: Arc<TestStore>) -> Self {
            Self {
                inner,
                prefetched: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn prefetched(&self) -> Vec<Vec<Key>> {
            self.prefetched
                .lock()
                .expect("prefetch recorder mutex should not be poisoned")
                .clone()
        }
    }

    impl KeyStore for RecordingFileStore {
        fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> Result<Option<Blob>> {
            self.inner.get_local_content(path, hgid)
        }

        fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
            self.prefetched
                .lock()
                .expect("prefetch recorder mutex should not be poisoned")
                .push(keys);
            Ok(())
        }

        fn clone_key_store(&self) -> Box<dyn KeyStore> {
            Box::new(self.clone())
        }
    }

    impl FileStore for RecordingFileStore {
        fn clone_file_store(&self) -> Box<dyn FileStore> {
            Box::new(self.clone())
        }
    }

    fn make_source_manifest(
        store: Arc<TestStore>,
        xml_hgid: HgId,
        project_path: &str,
        project_hgid: HgId,
    ) -> Result<TreeManifest> {
        let repo_xml_path: &RepoPath = REPO_XML_PATH.try_into()?;
        let xml = format!(
            "<manifest><project name=\"project\" path=\"{project_path}\" revision=\"{}\"/></manifest>",
            project_hgid.to_hex()
        );
        store.insert_data(
            InsertOpts {
                kind: Kind::File,
                forced_id: Some(Box::new(xml_hgid)),
                ..Default::default()
            },
            repo_xml_path,
            Blob::Bytes(xml.into_bytes().into()),
        )?;

        let mut manifest = TreeManifest::ephemeral(store.clone());
        manifest.insert(
            repo_path_buf(REPO_XML_PATH),
            FileMetadata::new(xml_hgid, FileType::Regular),
        )?;
        let root_hgid = manifest.persist(&[])?;
        Ok(TreeManifest::durable(store, root_hgid))
    }

    #[test]
    fn test_synthesize_grepo_projects_batch_prefetches_xml_blobs_once() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let first_xml_hgid = hgid("a1");
        let second_xml_hgid = hgid("a2");
        let first_project_hgid = hgid("b1");
        let second_project_hgid = hgid("b2");
        let manifests = vec![
            make_source_manifest(
                store.clone(),
                first_xml_hgid,
                "vendor/one",
                first_project_hgid,
            )?,
            make_source_manifest(
                store.clone(),
                second_xml_hgid,
                "vendor/two",
                second_project_hgid,
            )?,
        ];
        let recording_store = Arc::new(RecordingFileStore::new(store.clone()));
        let file_store: Arc<dyn FileStore> = recording_store.clone();
        let tree_store: Arc<dyn TreeStore> = store;

        let synthesized = synthesize_grepo_projects_batch(&tree_store, &file_store, manifests)?
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        assert_eq!(synthesized.len(), 2);
        assert_eq!(
            synthesized[0].get_file(repo_path("vendor/one"))?,
            Some(FileMetadata::new(
                first_project_hgid,
                FileType::GitSubmodule
            ))
        );
        assert_eq!(
            synthesized[1].get_file(repo_path("vendor/two"))?,
            Some(FileMetadata::new(
                second_project_hgid,
                FileType::GitSubmodule
            ))
        );
        assert_eq!(
            recording_store.prefetched(),
            vec![vec![
                Key::new(repo_path_buf(REPO_XML_PATH), first_xml_hgid),
                Key::new(repo_path_buf(REPO_XML_PATH), second_xml_hgid),
            ]]
        );
        Ok(())
    }

    #[test]
    fn test_synthesize_grepo_projects_batch_isolates_missing_xml() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let xml_hgid = hgid("a1");
        let project_hgid = hgid("b1");
        let mut missing = TreeManifest::ephemeral(store.clone());
        missing.insert(
            repo_path_buf("other"),
            FileMetadata::new(hgid("c1"), FileType::Regular),
        )?;
        let missing_root = missing.persist(&[])?;
        let manifests = vec![
            make_source_manifest(store.clone(), xml_hgid, "vendor/one", project_hgid)?,
            TreeManifest::durable(store.clone(), missing_root),
        ];
        let recording_store = Arc::new(RecordingFileStore::new(store.clone()));
        let file_store: Arc<dyn FileStore> = recording_store.clone();
        let tree_store: Arc<dyn TreeStore> = store;

        let synthesized = synthesize_grepo_projects_batch(&tree_store, &file_store, manifests)?;

        assert_eq!(synthesized.len(), 2);
        assert!(
            synthesized[0].is_ok(),
            "the manifest with XML should synthesize"
        );
        let error = synthesized[1]
            .as_ref()
            .expect_err("the manifest without XML should fail on its own");
        assert_eq!(
            error.to_string(),
            "repo manifest xml not found at static/static.xml in tree"
        );
        assert_eq!(
            recording_store.prefetched(),
            vec![vec![Key::new(repo_path_buf(REPO_XML_PATH), xml_hgid)]],
            "prefetch should skip the manifest without XML"
        );
        Ok(())
    }
}

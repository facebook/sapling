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
use storemodel::FileStore;
use storemodel::TreeStore;
use types::FetchContext;
use types::FileType;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

use crate::path::GrepoPathTranslator;

/// Parse .repo manifest xml files and synthesizes repo projects as trees.
/// TODO: linkfile and copyfile support
pub fn synthesize_grepo_projects(
    tree_store: &Arc<dyn TreeStore>,
    file_store: &Arc<dyn FileStore>,
    manifest: &TreeManifest,
) -> Result<TreeManifest> {
    let repo_xml_path: &RepoPath = "static/static.xml".try_into()?;
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

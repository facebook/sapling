/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Write project revisions from a commit into the repo manifest XML.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use grepomanifest::edit;
use grepomanifest::edit::Edit;
use grepomanifest::edit::Operation;
use grepomanifest::edit::Target;
use grepomanifest::parse::parse_manifest;
use grepomanifest::schema::Project;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest_tree::Manifest;
use manifest_tree::TreeManifest;
use storemodel::FileStore;
use storemodel::InsertOpts;
use types::FetchContext;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

/// Write new project revisions into the repo manifest XML.
///
/// Sapling only changes project revisions. The repo tool adds and removes
/// projects.
///
/// - `file_store`: the store for the XML file. The function reads the old XML
///   from it and writes the new XML to it.
/// - `repo_xml_path`: the path of the XML in `manifest_repo_tree`.
/// - `manifest_repo_tree`: the tree of the manifests repo (`.repo/manifests`).
///   It is not the synthesized tree of projects. The function writes the new
///   XML into this tree.
/// - `changes`: pairs of a project path and the new revision of that project.
///
/// Only the `revision` attributes of the XML change. Returns an error in these
/// cases:
/// - The tree has no XML.
/// - A path is not a project in the XML.
/// - A project has no `path`, and a different project has the same `name`.
pub fn apply_project_changes(
    file_store: &Arc<dyn FileStore>,
    repo_xml_path: &RepoPath,
    manifest_repo_tree: &mut TreeManifest,
    changes: &[(RepoPathBuf, HgId)],
) -> Result<()> {
    if changes.is_empty() {
        return Ok(());
    }

    let metadata = match manifest_repo_tree.get(repo_xml_path)? {
        Some(FsNodeMetadata::File(metadata)) => metadata,
        _ => bail!("repo manifest xml not found at {repo_xml_path} in tree"),
    };
    let mut xml_data = file_store
        .get_content(FetchContext::default(), repo_xml_path, metadata.hgid)?
        .into_bytes()
        .to_vec();
    let projects_from_xml = parse_manifest(&xml_data)?.projects;

    let edits = changes
        .iter()
        .map(|(path, revision)| {
            let project = projects_from_xml
                .get(&PathBuf::from(path.as_str()))
                .ok_or_else(|| {
                    anyhow!(
                        "{path} is not a project in {repo_xml_path}: use the repo tool to add projects"
                    )
                })?;
            Ok(Edit {
                target: find_target_for_project(project, &projects_from_xml)?,
                op: Operation::SetAttribute {
                    attr: "revision".to_string(),
                    value: revision.to_hex(),
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    edit::apply(&mut xml_data, &edits)?;

    let xml_hgid = file_store.insert_file(
        InsertOpts::default(),
        repo_xml_path,
        Blob::Bytes(xml_data.into()),
    )?;
    manifest_repo_tree.insert(
        repo_xml_path.to_owned(),
        FileMetadata::new(xml_hgid, metadata.file_type),
    )?;
    Ok(())
}

/// Return the selector for the `<project>` element of a project.
///
/// - `project`: the project to select.
/// - `projects_in_xml`: all projects parsed from the XML file.
///
/// If `project` has a `path`, the selector uses `name` and `path`. Else it uses
/// only `name`. Returns an error if `project` has no `path` and a different
/// project has the same `name`.
fn find_target_for_project(
    project: &Project,
    projects_in_xml: &BTreeMap<PathBuf, Project>,
) -> Result<Target> {
    let name = ("name".to_string(), project.name.clone());
    let attrs = match &project.path {
        Some(path) => vec![name, ("path".to_string(), path.display().to_string())],
        None if projects_in_xml
            .values()
            .filter(|p| p.name == project.name)
            .count()
            > 1 =>
        {
            bail!(
                "cannot select project {}: other projects have the same name",
                project.name
            );
        }
        None => vec![name],
    };
    Ok(Target {
        levels: vec![("project".to_string(), attrs)],
    })
}

#[cfg(test)]
mod tests {
    use manifest_tree::testutil::TestStore;
    use storemodel::KeyStore;
    use storemodel::Kind;
    use storemodel::TreeStore;
    use types::FileType;
    use types::testutil::hgid;
    use types::testutil::repo_path_buf;

    use super::*;

    const REPO_XML_PATH: &str = "static/static.xml";

    fn make_manifest(store: &Arc<TestStore>, xml: &str) -> Result<TreeManifest> {
        let xml_hgid = store.insert_data(
            InsertOpts {
                kind: Kind::File,
                ..Default::default()
            },
            REPO_XML_PATH.try_into()?,
            Blob::Bytes(xml.as_bytes().to_vec().into()),
        )?;
        let tree_store: Arc<dyn TreeStore> = store.clone();
        let mut manifest = TreeManifest::ephemeral(tree_store);
        manifest.insert(
            repo_path_buf(REPO_XML_PATH),
            FileMetadata::new(xml_hgid, FileType::Regular),
        )?;
        Ok(manifest)
    }

    fn read_xml(store: &Arc<TestStore>, manifest: &TreeManifest) -> Result<String> {
        let path: &RepoPath = REPO_XML_PATH.try_into()?;
        let metadata = manifest
            .get_file(path)?
            .ok_or_else(|| anyhow!("xml should be in the tree"))?;
        let data = store
            .get_content(FetchContext::default(), path, metadata.hgid)?
            .into_bytes();
        Ok(String::from_utf8(data.to_vec())?)
    }

    fn apply(store: &Arc<TestStore>, xml: &str, changes: &[(&str, HgId)]) -> Result<String> {
        let mut manifest = make_manifest(store, xml)?;
        let file_store: Arc<dyn FileStore> = store.clone();
        let changes = changes
            .iter()
            .map(|(path, revision)| (repo_path_buf(path), *revision))
            .collect::<Vec<_>>();
        apply_project_changes(
            &file_store,
            REPO_XML_PATH.try_into()?,
            &mut manifest,
            &changes,
        )?;
        read_xml(store, &manifest)
    }

    #[test]
    fn test_apply_project_changes_sets_only_revision() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let old = hgid("a1").to_hex();
        let new = hgid("b1");
        let xml = format!(
            "<manifest>\n  <!-- keep -->\n  <project name=\"a\" path=\"vendor/a\" revision=\"{old}\"/>\n  <project name=\"b\" path=\"vendor/b\" revision=\"main\"/>\n</manifest>\n"
        );

        let result = apply(&store, &xml, &[("vendor/b", new)])?;

        assert_eq!(
            result,
            xml.replace(
                "revision=\"main\"",
                &format!("revision=\"{}\"", new.to_hex())
            ),
            "only the revision of vendor/b should change"
        );
        Ok(())
    }

    #[test]
    fn test_apply_project_changes_matches_name_without_path() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let new = hgid("b1");
        let xml = "<manifest><project name=\"a\"/></manifest>";

        let result = apply(&store, xml, &[("a", new)])?;

        assert_eq!(
            result,
            format!(
                "<manifest><project name=\"a\" revision=\"{}\"/></manifest>",
                new.to_hex()
            )
        );
        Ok(())
    }

    #[test]
    fn test_apply_project_changes_rejects_unsupported_changes() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let xml = "<manifest><project name=\"a\" path=\"vendor/a\" revision=\"main\"/><project name=\"a\"/></manifest>";

        let errors = [("vendor/new", hgid("b1")), ("a", hgid("b1"))]
            .into_iter()
            .map(|change| {
                apply(&store, xml, &[change])
                    .expect_err("the change should be rejected")
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            errors,
            vec![
                "vendor/new is not a project in static/static.xml: use the repo tool to add projects",
                "cannot select project a: other projects have the same name",
            ]
        );
        Ok(())
    }

    #[test]
    fn test_apply_project_changes_rejects_tree_without_xml() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let tree_store: Arc<dyn TreeStore> = store.clone();
        let mut manifest = TreeManifest::ephemeral(tree_store);
        let file_store: Arc<dyn FileStore> = store.clone();

        let error = apply_project_changes(
            &file_store,
            REPO_XML_PATH.try_into()?,
            &mut manifest,
            &[(repo_path_buf("vendor/a"), hgid("b1"))],
        )
        .expect_err("a tree without the xml should be rejected");

        assert_eq!(
            error.to_string(),
            "repo manifest xml not found at static/static.xml in tree"
        );
        Ok(())
    }
}

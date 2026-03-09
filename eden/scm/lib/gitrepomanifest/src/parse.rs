/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use roxmltree::Document;
use roxmltree::Node;

use crate::schema::*;

/// Parse a manifest XML string into a `Manifest` struct.
#[allow(unused)]
pub fn parse_manifest(data: &[u8]) -> Result<Manifest> {
    let tree = get_tree(data)?;
    let root = tree.root_element();

    let mut manifest = Manifest::default();

    for element in root.children().filter(|n| n.is_element()) {
        match element.tag_name().name() {
            "project" => {
                let (path, project) = parse_project(&element)?;
                if manifest.projects.contains_key(&path) {
                    bail!("duplicate project path: {}", path.display());
                }
                manifest.projects.insert(path, project);
            }
            "remote" => {
                let (name, remote) = parse_remote(&element)?;
                if manifest.remotes.contains_key(&name) {
                    bail!("duplicate remote name: {}", name);
                }
                manifest.remotes.insert(name, remote);
            }
            "default" => manifest.default = Some(parse_default(&element)?),
            _ => {}
        }
    }

    Ok(manifest)
}

pub fn get_tree(data: &[u8]) -> Result<Document> {
    let text = str::from_utf8(data)?;
    let doc = roxmltree::Document::parse(text).context("roxmltree failed to parse manifest xml")?;
    Ok(doc)
}

fn attr<'a, T>(node: &'a roxmltree::Node, name: &str) -> Option<T>
where
    T: From<&'a str>,
{
    node.attribute(name).map(T::from)
}

fn parse_remote(node: &Node) -> Result<(String, Remote)> {
    let remote = Remote {
        name: attr(node, "name").expect("name is required for remote"),
        fetch: attr(node, "fetch").expect("fetch is required for remote"),
        alias: attr(node, "alias"),
        pushurl: attr(node, "pushurl"),
        review: attr(node, "review"),
        revision: attr(node, "revision"),
    };
    Ok((remote.name.clone(), remote))
}

fn parse_default(node: &Node) -> Result<Default> {
    let default = Default {
        remote: attr(node, "remote"),
        revision: attr(node, "revision"),
        dest_branch: attr(node, "dest-branch"),
        upstream: attr(node, "upstream"),
    };
    Ok(default)
}

fn parse_project(node: &Node) -> Result<(PathBuf, Project)> {
    let mut project = Project {
        name: attr(node, "name").expect("name is required for project"),
        path: attr(node, "path"),
        remote: attr(node, "remote"),
        revision: attr(node, "revision"),
        upstream: attr(node, "upstream"),
        linkfiles: Vec::new(),
        copyfiles: Vec::new(),
        annotations: Vec::new(),
    };
    for element in node.children().filter(|n| n.is_element()) {
        match element.tag_name().name() {
            "linkfile" => project.linkfiles.push(parse_linkfile(&element)?),
            "copyfile" => project.copyfiles.push(parse_copyfile(&element)?),
            "annotation" => project.annotations.push(parse_annotation(&element)?),
            "project" => anyhow::bail!("nested projects are not supported"),
            _ => {}
        }
    }

    // The path field is optional. If not supplied, the project "name" is used as path.
    let path = project
        .path
        .clone()
        .unwrap_or_else(|| PathBuf::from(&project.name));
    Ok((path, project))
}

fn parse_linkfile(node: &Node) -> Result<Linkfile> {
    let linkfile = Linkfile {
        src: attr(node, "src").expect("src is required for linkfile"),
        dest: attr(node, "dest").expect("dest is required for linkfile"),
    };
    Ok(linkfile)
}

fn parse_copyfile(node: &Node) -> Result<Copyfile> {
    let copyfile = Copyfile {
        src: attr(node, "src").expect("src is required for copyfile"),
        dest: attr(node, "dest").expect("dest is required for copyfile"),
    };
    Ok(copyfile)
}

fn parse_annotation(node: &Node) -> Result<Annotation> {
    let annotation = Annotation {
        name: attr(node, "name").expect("name is required for annotation"),
        value: attr(node, "value").expect("value is required for annotation"),
        keep: node.attribute("keep").map(|v| v == "true").unwrap_or(true),
    };
    Ok(annotation)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn test_parse_manifest_basic() {
        let data = r#"<?xml version="1.0"?>
<manifest>
  <remote name="origin" fetch="ssh://example.com"/>
  <default revision="main" remote="origin" sync-j="12"/>
  <project name="foo" path="src/foo" revision="abc" groups="dev"/>
  <project name="bar" path="src/bar" revision="def">
    <linkfile src="a" dest="b"/>
    <copyfile src="c" dest="d"/>
    <annotation name="key" value="val"/>
  </project>
</manifest>
"#;
        let m = parse_manifest(data.as_bytes()).unwrap();

        assert_eq!(m.remotes.len(), 1);
        let rorigin = m.remotes.get("origin").unwrap();
        assert_eq!(rorigin.name, "origin");
        assert_eq!(rorigin.fetch, "ssh://example.com");

        let d = m.default.unwrap();
        assert_eq!(d.revision.as_deref(), Some("main"));

        assert_eq!(m.projects.len(), 2);

        let pfoo = m.projects.get(Path::new("src/foo")).unwrap();
        assert_eq!(pfoo.name, "foo");
        assert!(pfoo.linkfiles.is_empty());

        let pbar = m.projects.get(Path::new("src/bar")).unwrap();
        assert_eq!(pbar.name, "bar");
        assert_eq!(pbar.linkfiles.len(), 1);
        assert_eq!(pbar.linkfiles[0].src.as_path(), "a");
        assert_eq!(pbar.linkfiles[0].dest.as_path(), "b");
        assert_eq!(pbar.copyfiles.len(), 1);
        assert_eq!(pbar.annotations.len(), 1);
        assert_eq!(pbar.annotations[0].name, "key");
        assert_eq!(pbar.annotations[0].value, "val");
        assert!(pbar.annotations[0].keep);
    }

    #[test]
    fn duplicate_project_path() {
        let data = br#"<?xml version="1.0"?>
<manifest>
  <project name="b" revision="abcabc"/>
  <project name="a" path="b" revision="cbacba"/>
</manifest>
"#;
        let err = parse_manifest(data).unwrap_err();
        assert!(err.to_string().contains("duplicate project path: b"));
    }
}

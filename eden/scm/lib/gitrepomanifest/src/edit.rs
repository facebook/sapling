/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Batched minimal-diff editing for repo manifest XML files.
//!
//! Editing is decoupled from parsing (parse.rs / schema.rs). The caller
//! accumulates semantic [`Edit`]s, then calls [`apply`] to splice
//! them into the raw XML data in place.

use std::ops::Range;
use std::str;

use anyhow::Result;
use roxmltree::Node;

use crate::parse::get_tree;

/// A byte-range replacement: replace `range` in the source with `data`.
/// Applied directly to xml content.
#[derive(Debug)]
struct Replace {
    range: Range<usize>,
    data: String,
}

pub type ElemName = String;

pub type AttrName = String;

pub type AttrVal = String;

/// Element selector, ordered from high level (closer to root) to low level (further from root).
/// `resolve_target` walks children at each level to find the target node.
pub struct Target {
    pub levels: Vec<(ElemName, Vec<(AttrName, AttrVal)>)>,
}

/// Operations to an element.
pub enum Operation {
    /// Set (or add) an attribute value on the matched element.
    SetAttribute { attr: AttrName, value: AttrVal },
    /// Remove the matched element entirely (including children and
    /// surrounding whitespace).
    RemoveElement,
    /// Add a self-closing child element inside the matched parent.
    AddChild {
        tag: ElemName,
        attrs: Vec<(AttrName, AttrVal)>,
    },
}

/// Find a `target` node, then perform `op`.
pub struct Edit {
    pub target: Target,
    pub op: Operation,
}

fn resolve(data: &[u8], edits: &[Edit]) -> Result<Vec<Replace>> {
    let src = str::from_utf8(data)?;
    let doc = get_tree(data)?;
    let root = doc.root_element();
    let mut repl = Vec::new();

    for edit in edits {
        let node = resolve_target(&root, &edit.target)?;
        repl.push(resolve_operation(src, &node, &edit.op)?);
    }

    Ok(repl)
}

fn resolve_target<'a, 'input>(
    root: &Node<'a, 'input>,
    target: &Target,
) -> Result<Node<'a, 'input>> {
    let mut current = *root;
    for (name, conditions) in &target.levels {
        current = current
            .children()
            .find(|n| {
                n.is_element()
                    && n.tag_name().name() == name
                    && conditions
                        .iter()
                        .all(|(k, v)| n.attribute(k.as_str()) == Some(v.as_str()))
            })
            .ok_or_else(|| anyhow::anyhow!("no <{}> matching {:?}", name, conditions))?;
    }
    Ok(current)
}

fn resolve_operation(src: &str, node: &Node, op: &Operation) -> Result<Replace> {
    match op {
        Operation::SetAttribute { attr, value } => resolve_set_attribute(src, node, attr, value),
        Operation::RemoveElement => unimplemented!(),
        Operation::AddChild { tag, attrs } => unimplemented!(),
    }
}

fn resolve_set_attribute(
    src: &str,
    node: &Node,
    attr_name: &str,
    attr_value: &str,
) -> Result<Replace> {
    assert!(node.is_element());
    if let Some(attr) = node.attributes().find(|a| a.name() == attr_name) {
        // Replace existing attribute value.
        Ok(Replace {
            range: attr.range_value(),
            data: attr_value.to_string(),
        })
    } else {
        // Attribute doesn't exist — insert before the `>` or `/>`.
        let pos = opening_tag_end(src, node)?;
        Ok(Replace {
            range: pos..pos,
            data: format!(" {}=\"{}\"", attr_name, attr_value),
        })
    }
}

/// Find the byte position of `>` or `/>` that closes the opening tag.
/// Returns the position just before `/` (if self-closing) or `>`.
fn opening_tag_end(src: &str, node: &Node) -> Result<usize> {
    let range = node.range();
    let tag_end_offset = src[range.start..range.end]
        .find('>')
        .ok_or_else(|| anyhow::anyhow!("cannot find tag end of an element"))?;
    let pos = range.start + tag_end_offset;
    if pos > 0 && src.as_bytes()[pos - 1] == b'/' {
        Ok(pos - 1)
    } else {
        Ok(pos)
    }
}

/// Apply semantic edits to `data` in place. Resolves edits to byte-range
/// replacements, then splices them in offset-descending order.
/// Caller needs to make sure target in each Edit leads to a unique element.
pub fn apply(data: &mut Vec<u8>, edits: &[Edit]) -> Result<()> {
    let mut repl = resolve(data, edits)?;
    repl.sort_by(|a, b| b.range.start.cmp(&a.range.start));
    for r in repl {
        data.splice(r.range, r.data.into_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = br#"<?xml version="1.0"?>
<manifest>
  <remote name="origin" fetch="ssh://example.com"/>
  <default revision="main" remote="origin"/>
  <project name="a" path="src/a" revision="abc123" groups="dev"/>
  <project name="b" path="src/b" revision="def456"/>
  <project name="c" path="src/c" revision="123456">
    <linkfile src="some/linksrc" dest="linkdest"/>
    <annotation name="prebuilt" value="true"/>
  </project>
  <project name="d" path="src/d" revision="abcdef"></project>
  <project name="e" path="src/e"/>
</manifest>
"#;

    fn run(data: &[u8], edits: Vec<Edit>) -> String {
        let mut buf = data.to_vec();
        apply(&mut buf, &edits).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_match_target() {
        let doc = get_tree(SAMPLE).unwrap();
        let root = doc.root_element();
        let target = Target {
            levels: vec![
                ("project".into(), vec![("path".into(), "src/c".into())]),
                ("linkfile".into(), vec![("dest".into(), "linkdest".into())]),
            ],
        };
        let node = resolve_target(&root, &target).unwrap();
        assert_eq!(node.attribute("src"), Some("some/linksrc"));
    }

    #[test]
    fn no_match_error() {
        let doc = get_tree(SAMPLE).unwrap();
        let root = doc.root_element();
        let target = Target {
            levels: vec![(
                "project".into(),
                vec![("path".into(), "nonexistent".into())],
            )],
        };
        let err = resolve_target(&root, &target).unwrap_err();
        assert!(err.to_string().contains("no <project>"));
    }

    #[test]
    fn set_attribute_existing() {
        let result = run(
            SAMPLE,
            vec![Edit {
                target: Target {
                    levels: vec![("project".into(), vec![("name".into(), "a".into())])],
                },
                op: Operation::SetAttribute {
                    attr: "revision".into(),
                    value: "aaaaaa".into(),
                },
            }],
        );
        assert!(
            result.contains(r#"  <project name="a" path="src/a" revision="aaaaaa" groups="dev"/>"#)
        );

        let result = run(
            SAMPLE,
            vec![Edit {
                target: Target {
                    levels: vec![
                        ("project".into(), vec![("path".into(), "src/c".into())]),
                        ("linkfile".into(), vec![("dest".into(), "linkdest".into())]),
                    ],
                },
                op: Operation::SetAttribute {
                    attr: "src".into(),
                    value: "new/linksrc".into(),
                },
            }],
        );
        assert!(result.contains(r#"    <linkfile src="new/linksrc" dest="linkdest"/>"#));
    }

    #[test]
    fn set_attribute_adds_when_missing() {
        let result = run(
            SAMPLE,
            vec![Edit {
                target: Target {
                    levels: vec![("project".into(), vec![("path".into(), "src/e".into())])],
                },
                op: Operation::SetAttribute {
                    attr: "revision".into(),
                    value: "eeeeee".into(),
                },
            }],
        );
        assert!(result.contains(r#"  <project name="e" path="src/e" revision="eeeeee"/>"#));
    }
}

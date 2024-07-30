/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use dag::dag::MemDag;
use dag::ops::ImportAscii;
use dag::set::SyncSetQuery;
use dag::DagAlgorithm;
use dag::Vertex;
use nonblocking::non_blocking_result;
use unicode_width::UnicodeWidthStr;

use super::render::Ancestor;
use super::render::Renderer;
use super::test_fixtures::TestFixture;

pub(crate) fn render_string(
    fixture: &TestFixture,
    renderer: &mut dyn Renderer<Vertex, Output = String>,
) -> String {
    render_string_with_order(fixture, renderer, None)
}

pub(crate) fn render_string_with_order(
    fixture: &TestFixture,
    renderer: &mut dyn Renderer<Vertex, Output = String>,
    order: Option<&[&str]>,
) -> String {
    let TestFixture {
        dag: ascii,
        messages,
        heads,
        reserve,
        ancestors,
        missing,
    } = fixture;
    let mut dag = MemDag::new();
    dag.import_ascii_with_heads(ascii, Some(heads)).unwrap();
    // str -> Vertex
    let v = |s: &str| Vertex::copy_from(s.as_bytes());

    let ancestors: HashSet<_> = ancestors
        .iter()
        .map(|(desc, anc)| (v(desc), v(anc)))
        .collect();
    let missing: HashSet<_> = missing.iter().map(|s| v(s)).collect();

    reserve
        .iter()
        .cloned()
        .map(v)
        .for_each(|s| renderer.reserve(s));

    let messages: HashMap<_, _> = messages.iter().cloned().collect();

    let iter: Vec<_> = match order {
        None => non_blocking_result(dag.all())
            .unwrap()
            .iter()
            .unwrap()
            .map(|v| v.unwrap())
            .collect(),
        Some(order) => order.iter().map(|name| v(name)).collect(),
    };

    let mut out = String::new();
    for node in iter {
        if missing.contains(&node) {
            continue;
        }
        let parents = non_blocking_result(dag.parent_names(node.clone()))
            .unwrap()
            .into_iter()
            .map(|parent| {
                if missing.contains(&parent) {
                    Ancestor::Anonymous
                } else if ancestors.contains(&(node.clone(), parent.clone())) {
                    Ancestor::Ancestor(parent)
                } else {
                    Ancestor::Parent(parent)
                }
            })
            .collect();
        let name = String::from_utf8(node.as_ref().to_vec()).unwrap();
        let message = match messages.get(name.as_str()) {
            Some(message) => format!("{}\n{}", name, message),
            None => name.clone(),
        };
        let width = renderer.width(Some(&node), Some(&parents));
        let row = renderer.next_row(node, parents, String::from("o"), message);
        let row_indent = row
            .lines()
            .filter_map(|line| line.find(&name).map(|offset| &line[..offset]))
            .next()
            .expect("name should be in the output");
        assert_eq!(
            row_indent.width() as u64,
            width,
            "indent '{}' for row for {} is the wrong width",
            row_indent,
            name
        );

        out.push_str(&row);
    }

    format!(
        "\n{}",
        out.trim_end()
            .lines()
            .map(|l| format!("            {}", l))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

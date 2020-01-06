/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use dag::{Group, Id, IdMap};
use tempfile::tempdir;

use crate::render::{Ancestor, Renderer};
use crate::test_fixtures::TestFixture;

pub(crate) fn render_string(
    fixture: &TestFixture,
    renderer: &mut dyn Renderer<Id, Output = String>,
) -> String {
    let TestFixture {
        dag,
        messages,
        heads,
        reserve,
        ancestors,
        missing,
    } = fixture;
    let dir = tempdir().unwrap();
    let mut id_map = IdMap::open(dir.path().join("id")).unwrap();
    let parents = drawdag::parse(dag);
    let parents_by_name = move |name: &[u8]| -> Result<Vec<Box<[u8]>>> {
        Ok(parents[&String::from_utf8(name.to_vec()).unwrap()]
            .iter()
            .map(|p| p.as_bytes().to_vec().into_boxed_slice())
            .collect())
    };

    let mut last_head = 0;
    for head in heads.iter() {
        id_map
            .assign_head(head.as_bytes(), &parents_by_name, Group::MASTER)
            .expect("can assign head");
        let Id(head_id) = id_map.find_id_by_slice(head.as_bytes()).unwrap().unwrap();
        last_head = head_id;
    }

    let ancestors: HashSet<_> = ancestors
        .iter()
        .map(|(desc, anc)| {
            (
                id_map.find_id_by_slice(desc.as_bytes()).unwrap().unwrap(),
                id_map.find_id_by_slice(anc.as_bytes()).unwrap().unwrap(),
            )
        })
        .collect();
    let missing: HashSet<_> = missing
        .iter()
        .map(|node| id_map.find_id_by_slice(node.as_bytes()).unwrap().unwrap())
        .collect();

    for reserve in reserve.iter() {
        let reserve_id = id_map
            .find_id_by_slice(reserve.as_bytes())
            .unwrap()
            .unwrap();
        renderer.reserve(reserve_id);
    }

    let parents_by_id = id_map.build_get_parents_by_id(&parents_by_name);
    let messages: HashMap<_, _> = messages.iter().cloned().collect();

    let mut out = String::new();
    for id in (0..=last_head).rev() {
        let node = Id(id);
        if missing.contains(&node) {
            continue;
        }
        let parents = parents_by_id(node)
            .unwrap()
            .into_iter()
            .map(|parent_id| {
                if missing.contains(&parent_id) {
                    Ancestor::Anonymous
                } else if ancestors.contains(&(node, parent_id)) {
                    Ancestor::Ancestor(parent_id)
                } else {
                    Ancestor::Parent(parent_id)
                }
            })
            .collect();
        let name =
            String::from_utf8(id_map.find_slice_by_id(node).unwrap().unwrap().to_vec()).unwrap();
        let message = match messages.get(name.as_str()) {
            Some(message) => format!("{}\n{}", name, message),
            None => name.clone(),
        };
        let row = renderer.next_row(node, parents, String::from("o"), message);
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

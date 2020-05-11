/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::render::{Ancestor, Renderer};
use anyhow::Result;
use dag::namedag::NameDagAlgorithm;
use dag::VertexName;

/// Render a NameDag or MemNameDag into a String.
pub fn render_namedag(
    dag: &impl NameDagAlgorithm,
    get_message: impl Fn(&VertexName) -> Option<String>,
) -> Result<String> {
    let mut renderer = crate::GraphRowRenderer::new().output().build_box_drawing();

    let iter: Vec<_> = dag.all()?.iter()?.collect::<Result<_>>()?;

    let mut out = String::new();
    for node in iter {
        let parents = dag
            .parent_names(node.clone())?
            .into_iter()
            .map(|parent| Ancestor::Parent(parent))
            .collect();
        let mut name = format!("{:?}", &node);
        let message = get_message(&node).unwrap_or(String::new());
        let row = if name.len() == 1 {
            renderer.next_row(node, parents, name, message)
        } else {
            if !message.is_empty() {
                name += &format!(" {}", message);
            }
            renderer.next_row(node, parents, String::from("o"), name)
        };
        out.push_str(&row);
    }

    let output = format!(
        "\n{}",
        out.trim_end()
            .lines()
            .map(|l| format!("            {}", l))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(output)
}

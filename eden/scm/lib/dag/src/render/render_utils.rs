/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::render::{Ancestor, Renderer};
use crate::DagAlgorithm;
use crate::VertexName;
use anyhow::Result;

/// Render a NameDag or MemNameDag into a String.
pub fn render_namedag(
    dag: &(impl DagAlgorithm + ?Sized),
    get_message: impl Fn(&VertexName) -> Option<String>,
) -> Result<String> {
    let mut renderer = super::GraphRowRenderer::new().output().build_box_drawing();

    let iter: Vec<_> = dag.all()?.iter()?.collect::<crate::Result<_>>()?;

    let mut out = String::new();
    for node in iter {
        let parents = dag
            .parent_names(node.clone())?
            .into_iter()
            .map(Ancestor::Parent)
            .collect();
        let mut name = format!("{:?}", &node);
        let message = get_message(&node).unwrap_or_default();
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

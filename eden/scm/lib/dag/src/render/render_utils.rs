/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(any(test, feature = "indexedlog-backend"))]
use std::cmp::Ordering;
#[cfg(any(test, feature = "indexedlog-backend"))]
use std::io::Write;

use anyhow::Result;
use nonblocking::non_blocking_result;

use super::render::Ancestor;
use super::render::Renderer;
use crate::nameset::SyncNameSetQuery;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::ops::IdConvert;
use crate::DagAlgorithm;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::Group;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::IdSpan;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::Level;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::NameDag;
use crate::VertexName;

/// Render a NameDag or MemNameDag into a String.
pub fn render_namedag(
    dag: &(impl DagAlgorithm + ?Sized),
    get_message: impl Fn(&VertexName) -> Option<String>,
) -> Result<String> {
    let mut renderer = super::GraphRowRenderer::new().output().build_box_drawing();

    let iter: Vec<_> = non_blocking_result(dag.all())?
        .iter()?
        .collect::<crate::Result<_>>()?;

    let mut out = String::new();
    for node in iter {
        let parents = non_blocking_result(dag.parent_names(node.clone()))?
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

#[cfg(any(test, feature = "indexedlog-backend"))]
pub fn render_segment_dag(
    mut out: impl Write,
    dag: &NameDag,
    level: Level,
    group: Group,
) -> Result<()> {
    let mut renderer = super::GraphRowRenderer::new().output().build_box_drawing();
    let segs = dag.dag.next_segments(group.min_id(), level)?;

    for seg in segs.iter().rev() {
        let mut parents = vec![];
        for parent_id in seg.parents()? {
            // For each parent Id, look for the containing segment.
            let parent_span: IdSpan = parent_id.into();
            let parent_idx = segs.binary_search_by(|s| {
                let span = s.span().unwrap();
                if span.contains(parent_id) {
                    Ordering::Equal
                } else {
                    span.cmp(&parent_span)
                }
            });

            if let Ok(parent_idx) = parent_idx {
                parents.push(Ancestor::Parent(&segs[parent_idx]));
            } else {
                // Probably a non-master segment with master parent.
                parents.push(Ancestor::Anonymous);
            }
        }

        let span = seg.span()?;
        let get_hex = |id| -> String {
            non_blocking_result(dag.vertex_name(id))
                .map(|s| format!("{:.12?}", s))
                .unwrap_or_default()
        };
        let name = format!(
            "{}({})-{}({})",
            get_hex(span.low),
            span.low,
            get_hex(span.high),
            span.high,
        );
        let row = renderer.next_row(seg, parents, String::from("o"), name);
        write!(out, "{}", row)?;
    }

    Ok(())
}

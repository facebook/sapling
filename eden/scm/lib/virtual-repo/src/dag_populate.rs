/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag::CloneData;
use dag::Dag;
use dag::FlatSegment;
use dag::Group;
use dag::Id;
use dag::PreparedFlatSegments;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::ops::DagImportPullData;
use types::Id20;

use crate::id_fields::IdFields;
use crate::id_fields::ObjectKind;
use crate::provider::get_tree_provider;

/// Populate `dag` so it has commit history for `factor_bits` virtual-repo commits.
/// See [`IdFields`] for approximate repo sizes with different `factor_bits`.
///
/// Returns the "head" commit hash.
///
/// The `dag` must have [`crate::dag_protocol::VirtualIdConvertProtocol`] set as
/// "remote protocol" to access the populated commits.
pub async fn populate_dag(dag: &mut Dag, factor_bits: u8) -> dag::Result<Vertex> {
    let high_id = {
        let tree_provider = get_tree_provider(factor_bits);
        let commit_len = tree_provider.root_tree_len();
        commit_len.saturating_sub(1) as u64
    };

    let high_id_fields = IdFields {
        kind: ObjectKind::Commit,
        factor_bits,
        id8: high_id,
    };
    let head_vertex = high_id_fields.to_vertex();
    let heads: VertexListWithOptions =
        VertexListWithOptions::from(vec![head_vertex.clone()]).with_desired_group(Group::MASTER);

    let pull_data: CloneData<Vertex> = {
        let low = Id(0);
        let high = Id(high_id);
        let segment = FlatSegment {
            low,
            high,
            parents: Vec::new(),
        };
        let flat_segments = PreparedFlatSegments {
            segments: std::iter::once(segment).collect(),
        };
        let low_vertex = IdFields {
            id8: 0,
            ..high_id_fields
        }
        .to_vertex();
        let idmap = [(low, low_vertex), (high, head_vertex.clone())]
            .into_iter()
            .collect();
        CloneData {
            flat_segments,
            idmap,
        }
    };

    dag.import_pull_data(pull_data, &heads).await?;
    Ok(head_vertex)
}

impl IdFields {
    fn to_vertex(self) -> Vertex {
        let id20 = Id20::from(self);
        Vertex::copy_from(id20.as_ref())
    }
}

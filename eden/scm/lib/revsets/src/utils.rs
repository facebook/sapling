/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::ops::IdConvert;
use dag::VertexName;

use crate::errors::RevsetLookupError;

pub fn resolve_single(
    change_id: &str,
    id_map: &dyn IdConvert,
) -> Result<VertexName, RevsetLookupError> {
    let mut vertices = async_runtime::block_on(async {
        id_map.vertexes_by_hex_prefix(change_id.as_bytes(), 5).await
    })?
    .into_iter();

    let vertex = if let Some(v) = vertices.next() {
        v
    } else {
        return Err(RevsetLookupError::RevsetNotFound(change_id.to_owned()));
    };

    if let Some(vertex2) = vertices.next() {
        let mut possible_identifiers = vec![vertex.to_hex(), vertex2.to_hex()];
        for vertex in vertices {
            possible_identifiers.push(vertex.to_hex());
        }
        return Err(RevsetLookupError::AmbiguousIdentifier(
            change_id.to_owned(),
            possible_identifiers.join(", "),
        ));
    }

    Ok(vertex)
}

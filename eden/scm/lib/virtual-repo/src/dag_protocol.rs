/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use async_trait::async_trait;
use dag::Vertex;
use dag::protocol::AncestorPath;
use dag::protocol::RemoteIdConvertProtocol;
use types::Id20;

use crate::id_fields::IdFields;
use crate::id_fields::ObjectKind;

#[derive(Clone, Copy)]
pub(crate) struct VirtualIdConvertProtocol;

impl IdFields {
    fn maybe_from_vertex_as_commit(v: &Vertex) -> Option<Self> {
        let id20 = Id20::from_slice(v.as_ref()).ok()?;
        let fields = Self::maybe_from_id20(id20)?;
        match fields.kind {
            ObjectKind::Commit => Some(fields),
            _ => None,
        }
    }
}

#[async_trait]
impl RemoteIdConvertProtocol for VirtualIdConvertProtocol {
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<Vertex>,
        names: Vec<Vertex>,
    ) -> dag::Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        // We need to reason about vertexes using `IdFields`.
        let resolved_heads: Vec<(IdFields, Vertex)> = heads
            .into_iter()
            .filter_map(|head| {
                let resolved = IdFields::maybe_from_vertex_as_commit(&head)?;
                Some((resolved, head))
            })
            .collect();

        let result: Vec<(AncestorPath, Vec<Vertex>)> = names
            .iter()
            .filter_map(|name| {
                let resolved = IdFields::maybe_from_vertex_as_commit(name)?;
                let path = resolved_heads
                    .iter()
                    .filter_map(|(resolved_head, head)| {
                        if resolved_head.is_compatible_with(&resolved)
                            && resolved.id8 < resolved_head.id8
                        {
                            let n = resolved_head.id8 - resolved.id8;
                            let path = AncestorPath {
                                x: head.clone(),
                                n,
                                // NOTE: Can be smarter by using batch_size > 1 and returning more
                                // than 1 vertexes. For now, uses the simpler but correct form.
                                batch_size: 1,
                            };
                            Some(path)
                        } else {
                            None
                        }
                    })
                    .next()?;
                Some((path, vec![name.clone()]))
            })
            .collect();

        Ok(result)
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> dag::Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        // Technically, failures are unexpected (i.e. use `map` instead of `filter_map`).
        // Practically, `?` with `Option`s are easier (shorter) to write.
        let result = paths
            .into_iter()
            .filter_map(|path| {
                let resolved_head = IdFields::maybe_from_vertex_as_commit(&path.x)?;
                let mut resolved_vertexes = Vec::with_capacity(path.batch_size as _);
                for i in 0..path.batch_size {
                    let new_id8 = resolved_head.id8.checked_sub(path.n + i)?;
                    let resolved = resolved_head.with_kind_id8(resolved_head.kind, new_id8);
                    let resolved = Id20::from(resolved);
                    let vertex = Vertex::copy_from(resolved.as_ref());
                    resolved_vertexes.push(vertex);
                }
                Some((path, resolved_vertexes))
            })
            .collect();
        Ok(result)
    }
}

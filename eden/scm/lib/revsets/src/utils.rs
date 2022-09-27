/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use dag::ops::IdConvert;
use dag::VertexName;
use metalog::MetaLog;
use refencode::decode_bookmarks;
use refencode::decode_remotenames;
use treestate::treestate::TreeState;
use types::HgId;

use crate::errors::RevsetLookupError;

pub fn resolve_single(
    change_id: &str,
    id_map: &dyn IdConvert,
    metalog: &MetaLog,
    treestate: &TreeState,
) -> Result<String, RevsetLookupError> {
    if let Some(vertex) = resolve_dot(change_id, treestate)? {
        return Ok(vertex.to_hex());
    }
    if let Some(bookmark) = resolve_bookmark(change_id, metalog)? {
        return Ok(bookmark.to_hex());
    }
    if let Some(vertex) = resolve_hash_prefix(change_id, id_map)? {
        return Ok(vertex.to_hex());
    }

    Err(RevsetLookupError::RevsetNotFound(change_id.to_owned()))
}

pub fn resolve_dot(
    change_id: &str,
    treestate: &TreeState,
) -> Result<Option<HgId>, RevsetLookupError> {
    if change_id != "." && !change_id.is_empty() {
        return Ok(None);
    }
    treestate.parents().next().map_or_else(
        || Ok(Some(HgId::null_id().clone())),
        |first_commit| {
            first_commit.map_or_else(
                |err| Err(RevsetLookupError::TreeStateError(err)),
                |c| Ok(Some(c)),
            )
        },
    )
}

fn resolve_hash_prefix(
    change_id: &str,
    id_map: &dyn IdConvert,
) -> Result<Option<VertexName>, RevsetLookupError> {
    if !change_id
        .chars()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Ok(None);
    }
    let mut vertices = async_runtime::block_on(async {
        id_map.vertexes_by_hex_prefix(change_id.as_bytes(), 5).await
    })?
    .into_iter();

    let vertex = if let Some(v) = vertices.next() {
        v
    } else {
        return Ok(None);
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

    Ok(Some(vertex))
}

fn resolve_bookmark(change_id: &str, metalog: &MetaLog) -> Result<Option<HgId>, RevsetLookupError> {
    if let Some(hash) = resolve_metalog_bookmark(change_id, metalog, "bookmarks", decode_bookmarks)?
    {
        return Ok(Some(hash));
    }
    if let Some(hash) =
        resolve_metalog_bookmark(change_id, metalog, "remotenames", decode_remotenames)?
    {
        return Ok(Some(hash));
    }
    Ok(None)
}

fn resolve_metalog_bookmark(
    change_id: &str,
    metalog: &MetaLog,
    bookmark_type: &str,
    decoder: fn(&[u8]) -> std::io::Result<BTreeMap<String, HgId>>,
) -> Result<Option<HgId>, RevsetLookupError> {
    let raw_bookmarks = match metalog.get(bookmark_type)? {
        None => {
            return Ok(None);
        }
        Some(raw_bookmarks) => raw_bookmarks.into_vec(),
    };
    let mut bookmark_map = decoder(raw_bookmarks.as_slice()).map_err(|err| {
        RevsetLookupError::BookmarkDecodeError(change_id.to_owned(), bookmark_type.to_owned(), err)
    })?;
    Ok(bookmark_map.remove(change_id))
}

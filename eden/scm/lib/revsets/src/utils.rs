/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;

use dag::ops::IdConvert;
use metalog::MetaLog;
use refencode::decode_bookmarks;
use refencode::decode_remotenames;
use treestate::treestate::TreeState;
use types::HgId;

use crate::errors::RevsetLookupError;

struct LookupArgs<'a> {
    change_id: &'a str,
    id_map: &'a dyn IdConvert,
    metalog: &'a MetaLog,
    treestate: &'a TreeState,
}

pub fn resolve_single(
    change_id: &str,
    id_map: &dyn IdConvert,
    metalog: &MetaLog,
    treestate: &TreeState,
) -> Result<HgId, RevsetLookupError> {
    let args = LookupArgs {
        change_id,
        id_map,
        metalog,
        treestate,
    };
    let fns = [
        resolve_special,
        resolve_dot,
        resolve_bookmark,
        resolve_hash_prefix,
    ];

    for f in fns.iter() {
        if let Some(r) = f(&args)? {
            return Ok(r);
        }
    }

    Err(RevsetLookupError::RevsetNotFound(change_id.to_owned()))
}

fn resolve_special(args: &LookupArgs) -> Result<Option<HgId>, RevsetLookupError> {
    if args.change_id == "null" {
        return Ok(Some(HgId::null_id().clone()));
    }
    if args.change_id != "tip" {
        return Ok(None);
    }
    args.metalog
        .get(args.change_id)?
        .map(|tip| {
            HgId::from_slice(&tip).map_err(|err| {
                let tip = String::from_utf8_lossy(&tip).to_string();
                RevsetLookupError::CommitHexParseError(tip, err.into())
            })
        })
        .transpose()
}

fn resolve_dot(args: &LookupArgs) -> Result<Option<HgId>, RevsetLookupError> {
    if args.change_id != "." && !args.change_id.is_empty() {
        return Ok(None);
    }
    args.treestate.parents().next().map_or_else(
        || Ok(Some(HgId::null_id().clone())),
        |first_commit| {
            first_commit.map_or_else(
                |err| Err(RevsetLookupError::TreeStateError(err)),
                |c| Ok(Some(c)),
            )
        },
    )
}

fn resolve_hash_prefix(args: &LookupArgs) -> Result<Option<HgId>, RevsetLookupError> {
    if !args
        .change_id
        .chars()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Ok(None);
    }
    let mut vertices = async_runtime::block_on(async {
        args.id_map
            .vertexes_by_hex_prefix(args.change_id.as_bytes(), 5)
            .await
    })?
    .into_iter();

    let vertex = if let Some(v) = vertices.next() {
        v.to_hex()
    } else {
        return Ok(None);
    };

    if let Some(vertex2) = vertices.next() {
        let mut possible_identifiers = vec![vertex, vertex2.to_hex()];
        for vertex in vertices {
            possible_identifiers.push(vertex.to_hex());
        }
        return Err(RevsetLookupError::AmbiguousIdentifier(
            args.change_id.to_owned(),
            possible_identifiers.join(", "),
        ));
    }

    Ok(Some(HgId::from_str(&vertex).map_err(|err| {
        RevsetLookupError::CommitHexParseError(vertex, err.into())
    })?))
}

fn resolve_bookmark(args: &LookupArgs) -> Result<Option<HgId>, RevsetLookupError> {
    if let Some(hash) =
        resolve_metalog_bookmark(args.change_id, args.metalog, "bookmarks", decode_bookmarks)?
    {
        return Ok(Some(hash));
    }
    if let Some(hash) = resolve_metalog_bookmark(
        args.change_id,
        args.metalog,
        "remotenames",
        decode_remotenames,
    )? {
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

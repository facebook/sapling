/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use configmodel::Config;
use dag::ops::IdConvert;
use dag::Vertex;
use edenapi::EdenApi;
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
    treestate: Option<&'a TreeState>,
    config: &'a dyn Config,
    edenapi: Option<&'a dyn EdenApi>,
}

pub fn resolve_single(
    config: &dyn Config,
    change_id: &str,
    id_map: &dyn IdConvert,
    metalog: &MetaLog,
    treestate: Option<&TreeState>,
    edenapi: Option<&dyn EdenApi>,
) -> Result<HgId> {
    let args = LookupArgs {
        config,
        change_id,
        id_map,
        metalog,
        treestate,
        edenapi,
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

    Err(RevsetLookupError::RevsetNotFound(change_id.to_owned()).into())
}

fn resolve_special(args: &LookupArgs) -> Result<Option<HgId>> {
    if args.change_id == "null" {
        return Ok(Some(HgId::null_id().clone()));
    }
    if args.change_id != "tip" {
        return Ok(None);
    }
    args.metalog
        .get(args.change_id)?
        .map(|tip| {
            if tip.is_empty() {
                Ok(HgId::null_id().clone())
            } else {
                HgId::from_slice(&tip).map_err(|err| {
                    let tip = String::from_utf8_lossy(&tip).to_string();
                    RevsetLookupError::CommitHexParseError(tip, err.into()).into()
                })
            }
        })
        .transpose()
}

fn resolve_dot(args: &LookupArgs) -> Result<Option<HgId>> {
    if args.change_id != "." && !args.change_id.is_empty() {
        return Ok(None);
    }

    match args.treestate {
        Some(treestate) => match treestate.parents().next() {
            None => Ok(Some(HgId::null_id().clone())),
            Some(hgid) => Ok(Some(hgid?)),
        },
        None => Ok(None),
    }
}

fn resolve_hash_prefix(args: &LookupArgs) -> Result<Option<HgId>> {
    let change_id = args.change_id;

    if !change_id
        .chars()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Ok(None);
    }

    let hgid = match change_id.len() {
        l if l > 40 => return Ok(None),
        l if l == 40 => HgId::from_hex(change_id.as_bytes())?,
        _ => {
            if let Some(id) = local_hash_prefix_lookup(args)? {
                // We found it locally - no need to do any more work.
                return Ok(Some(id));
            }

            match remote_hash_prefix_lookup(args)? {
                Some(id) => id,
                None => return Ok(None),
            }
        }
    };

    if async_runtime::block_on(async {
        args.id_map
            .contains_vertex_name(&Vertex::copy_from(hgid.as_ref()))
            .await
    })? {
        Ok(Some(hgid))
    } else {
        Ok(None)
    }
}

fn local_hash_prefix_lookup(args: &LookupArgs) -> Result<Option<HgId>> {
    let hgids = async_runtime::block_on(async {
        args.id_map
            .vertexes_by_hex_prefix(args.change_id.as_bytes(), 5)
            .await
    })?
    .into_iter()
    .map(|v| {
        Ok(HgId::from_byte_array(v.0.into_vec().try_into().map_err(
            |v| anyhow!("unexpected vertex name length: {:?}", v),
        )?))
    })
    .collect::<Result<Vec<_>>>()?;

    error_if_ambiguous(args.change_id, hgids)
}

fn remote_hash_prefix_lookup(args: &LookupArgs) -> Result<Option<HgId>> {
    let edenapi = match args.edenapi {
        Some(edenapi) => edenapi,
        None => return Ok(None),
    };

    let mut response = async_runtime::block_on(async {
        edenapi
            .hash_prefixes_lookup(vec![args.change_id.to_string()])
            .await
    })?;

    let hgids = response.pop().map(|r| r.hgids).unwrap_or_default();

    if !response.is_empty() {
        bail!("unexpected hash_prefixes_lookup response");
    }

    error_if_ambiguous(args.change_id, hgids)
}

fn error_if_ambiguous(input: &str, hgids: Vec<HgId>) -> Result<Option<HgId>> {
    if hgids.is_empty() {
        Ok(None)
    } else if hgids.len() == 1 {
        Ok(Some(hgids[0]))
    } else {
        Err(RevsetLookupError::AmbiguousIdentifier(
            input.to_owned(),
            hgids
                .into_iter()
                .map(|v| v.to_hex())
                .collect::<Vec<_>>()
                .join(", "),
        )
        .into())
    }
}

fn resolve_bookmark(args: &LookupArgs) -> Result<Option<HgId>> {
    let mut local_bookmarks = metalog_bookmarks(args.metalog, "bookmarks", decode_bookmarks)?;
    if let Some(hash) = local_bookmarks.remove(args.change_id) {
        return Ok(Some(hash));
    }

    let mut remote_bookmarks = metalog_bookmarks(args.metalog, "remotenames", decode_remotenames)?;
    if let Some(hash) = remote_bookmarks.remove(args.change_id) {
        return Ok(Some(hash));
    }

    if let Some(hoist) = args.config.get("remotenames", "hoist") {
        if let Some(hash) = remote_bookmarks.remove(&format!("{}/{}", hoist, args.change_id)) {
            return Ok(Some(hash));
        }
    }

    Ok(None)
}

fn metalog_bookmarks(
    metalog: &MetaLog,
    bookmark_type: &str,
    decoder: fn(&[u8]) -> std::io::Result<BTreeMap<String, HgId>>,
) -> Result<BTreeMap<String, HgId>> {
    let raw_bookmarks = match metalog.get(bookmark_type)? {
        None => {
            return Ok(Default::default());
        }
        Some(raw_bookmarks) => raw_bookmarks.into_vec(),
    };

    Ok(decoder(raw_bookmarks.as_slice())
        .map_err(|err| RevsetLookupError::BookmarkDecodeError(bookmark_type.to_owned(), err))?)
}

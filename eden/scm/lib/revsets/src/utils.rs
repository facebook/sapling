/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use async_runtime::block_on;
use configmodel::Config;
use configmodel::ConfigExt;
use dag::DagAlgorithm;
use dag::Id;
use dag::Vertex;
use dag::ops::IdConvert;
use edenapi::SaplingRemoteApi;
use metalog::MetaLog;
use treestate::treestate::TreeState;
use types::HgId;
use types::hgid::NULL_ID;
use types::hgid::WDIR_ID;
use types::hgid::WDIR_REV;

use crate::errors::RevsetLookupError;

struct LookupArgs<'a> {
    change_id: &'a str,
    id_map: &'a dyn IdConvert,
    dag: &'a dyn DagAlgorithm,
    metalog: &'a MetaLog,
    treestate: Option<&'a TreeState>,
    config: &'a dyn Config,
    edenapi: Option<&'a dyn SaplingRemoteApi>,
}

pub fn resolve_single(
    config: &dyn Config,
    change_id: &str,
    id_map: &dyn IdConvert,
    dag: &dyn DagAlgorithm,
    metalog: &MetaLog,
    treestate: Option<&TreeState>,
    edenapi: Option<&dyn SaplingRemoteApi>,
) -> Result<HgId> {
    let args = LookupArgs {
        config,
        change_id,
        id_map,
        dag,
        metalog,
        treestate,
        edenapi,
    };
    let fns = [
        resolve_special,
        resolve_dot,
        resolve_revnum,
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

    if let Some(tip) = args.metalog.get(args.change_id)? {
        if block_on(async { args.id_map.contains_vertex_name(&tip.clone().into()).await })? {
            return Ok(Some(HgId::from_slice(&tip).context("metalog tip")?));
        }
    }

    Ok(Some(
        block_on(async { args.dag.all().await?.first().await })?
            .map_or_else(|| Ok(NULL_ID), |v| HgId::from_slice(v.as_ref()))?,
    ))
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
        40 => HgId::from_hex(change_id.as_bytes())?,
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

    if block_on(async {
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
    let hgids = block_on(async {
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

    let mut response = block_on(async {
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
    let mut local_bookmarks = args.metalog.get_bookmarks()?;
    if let Some(hash) = local_bookmarks.remove(args.change_id) {
        return Ok(Some(hash));
    }

    let mut remote_bookmarks = args.metalog.get_remotenames()?;
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

fn resolve_revnum(args: &LookupArgs) -> Result<Option<HgId>> {
    if !hgplain::is_plain(None) && args.config.get_or_default("ui", "ignorerevnum")? {
        return Ok(None);
    }

    let rev: i64 = match args.change_id.parse() {
        Err(_) => return Ok(None),
        Ok(rev) => rev,
    };

    let id = match rev {
        -1 => NULL_ID,
        WDIR_REV => WDIR_ID,
        rev => {
            let name = block_on(async { args.id_map.vertex_name(Id(rev as u64)).await })?;
            HgId::from_byte_array(
                name.0
                    .into_vec()
                    .try_into()
                    .map_err(|v| anyhow!("unexpected vertex name length: {:?}", v))?,
            )
        }
    };

    if args.config.get("devel", "legacy.revnum").as_deref() == Some("abort") {
        bail!("local revision number is disabled in this repo");
    }

    Ok(Some(id))
}

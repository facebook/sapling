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
use types::HgId;
use types::hgid::NULL_ID;
use types::hgid::WDIR_ID;
use types::hgid::WDIR_REV;

use crate::errors::RevsetLookupError;

/// Result of resolving a single commit identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    /// The identifier could not be resolved.
    NotFound(String),
    /// The identifier was resolved and exists in the local dag.
    Local(String, HgId),
    /// The identifier was resolved via remote lookup, but the commit
    /// is not yet in the local dag.
    RemoteOnly(String, HgId),
}

impl ResolveResult {
    /// Returns the HgId if resolved locally, or an error otherwise.
    pub fn local(self) -> Result<HgId> {
        match self {
            ResolveResult::Local(_, id) => Ok(id),
            ResolveResult::RemoteOnly(name, _) | ResolveResult::NotFound(name) => {
                Err(RevsetLookupError::RevsetNotFound(name).into())
            }
        }
    }

    /// Returns the HgId if resolved (local or remote), or an error if not found.
    pub fn any(self) -> Result<HgId> {
        match self {
            ResolveResult::Local(_, id) | ResolveResult::RemoteOnly(_, id) => Ok(id),
            ResolveResult::NotFound(name) => Err(RevsetLookupError::RevsetNotFound(name).into()),
        }
    }
}

struct LookupArgs<'a> {
    change_id: &'a str,
    id_map: &'a dyn IdConvert,
    dag: &'a dyn DagAlgorithm,
    metalog: &'a MetaLog,
    working_copy_p1: Option<HgId>,
    config: &'a dyn Config,
    edenapi: Option<&'a dyn SaplingRemoteApi>,
}

pub fn resolve_single(
    config: &dyn Config,
    change_id: &str,
    id_map: &dyn IdConvert,
    dag: &dyn DagAlgorithm,
    metalog: &MetaLog,
    working_copy_p1: Option<HgId>,
    edenapi: Option<&dyn SaplingRemoteApi>,
) -> Result<ResolveResult> {
    let args = LookupArgs {
        config,
        change_id,
        id_map,
        dag,
        metalog,
        working_copy_p1,
        edenapi,
    };

    // Try local-only resolution functions first.
    let local_fns = [
        resolve_special,
        resolve_dot,
        resolve_revnum,
        resolve_bookmark,
    ];

    for f in local_fns.iter() {
        if let Some(r) = f(&args)? {
            return Ok(ResolveResult::Local(change_id.to_string(), r));
        }
    }

    // Hash prefix resolution can return local or remote-only results.
    resolve_hash_prefix(&args)
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

    Ok(args.working_copy_p1)
}

fn resolve_hash_prefix(args: &LookupArgs) -> Result<ResolveResult> {
    let change_id = args.change_id;

    if !change_id
        .chars()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Ok(ResolveResult::NotFound(change_id.to_string()));
    }

    match change_id.len() {
        l if l > 40 => Ok(ResolveResult::NotFound(change_id.to_string())),
        40 => {
            let hgid = HgId::from_hex(change_id.as_bytes())?;
            Ok(block_on(async {
                if args
                    .id_map
                    .contains_vertex_name(&Vertex::copy_from(hgid.as_ref()))
                    .await?
                {
                    anyhow::Ok(ResolveResult::Local(change_id.to_string(), hgid))
                } else if let Some(api) = args.edenapi {
                    let known = api
                        .commit_known(vec![hgid])
                        .await?
                        .into_iter()
                        .next()
                        .map(|r| r.known)
                        .transpose()?
                        .unwrap_or(false);
                    if known {
                        Ok(ResolveResult::RemoteOnly(change_id.to_string(), hgid))
                    } else {
                        Ok(ResolveResult::NotFound(change_id.to_string()))
                    }
                } else {
                    Ok(ResolveResult::NotFound(change_id.to_string()))
                }
            })?)
        }
        _ => {
            // Try local lookup first.
            if let Some(id) = local_hash_prefix_lookup(args)? {
                return Ok(ResolveResult::Local(change_id.to_string(), id));
            }

            // Try remote lookup.
            match args
                .edenapi
                .and_then(|api| remote_hash_prefix_lookup(api, args.change_id).transpose())
            {
                Some(Ok(id)) => {
                    // Found remotely - check if it's also in the local dag.
                    if block_on(async {
                        args.id_map
                            .contains_vertex_name(&Vertex::copy_from(id.as_ref()))
                            .await
                    })? {
                        Ok(ResolveResult::Local(change_id.to_string(), id))
                    } else {
                        Ok(ResolveResult::RemoteOnly(change_id.to_string(), id))
                    }
                }
                Some(Err(e)) => Err(e),
                None => Ok(ResolveResult::NotFound(change_id.to_string())),
            }
        }
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

pub fn remote_hash_prefix_lookup(slapi: &dyn SaplingRemoteApi, id: &str) -> Result<Option<HgId>> {
    let mut response = block_on(async { slapi.hash_prefixes_lookup(vec![id.to_string()]).await })?;

    let hgids = response.pop().map(|r| r.hgids).unwrap_or_default();

    if !response.is_empty() {
        bail!("unexpected hash_prefixes_lookup response");
    }

    error_if_ambiguous(id, hgids)
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

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # namedag
//!
//! Combination of IdMap and IdDag.

use crate::id::Group;
use crate::id::VertexName;
use crate::idmap::IdMap;
use crate::idmap::IdMapLike;
use crate::idmap::SyncableIdMap;
use crate::segment::IdDag;
use crate::segment::SyncableIdDag;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A DAG that uses VertexName instead of ids as vertexes.
///
/// A high-level wrapper structure. Combination of [`IdMap`] and [`Dag`].
/// Maintains consistency of dag and map internally.
pub struct NameDag {
    pub(crate) dag: IdDag,
    pub(crate) map: IdMap,
}

impl NameDag {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut map = IdMap::open(path.join("idmap"))?;
        // Take a lock so map and dag are loaded consistently.  A better (lock-free) way to ensure
        // this is to use a single "meta" file for both indexedlogs. However that requires some
        // API changes on the indexedlog side.
        let _locked = map.prepare_filesystem_sync()?;
        map.reload()?;
        let dag = IdDag::open(path.join("segments"))?;
        Ok(Self { dag, map })
    }

    /// Build segments. Write to disk.
    pub fn build<F>(
        &mut self,
        parent_names_func: F,
        master_names: &[VertexName],
        non_master_names: &[VertexName],
    ) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
    {
        // Already include specified nodes?
        if master_names.iter().all(|n| {
            is_ok_some(
                self.map
                    .find_id_by_name_with_max_group(n.as_ref(), Group::MASTER),
            )
        }) && non_master_names
            .iter()
            .all(|n| is_ok_some(self.map.find_id_by_name(n.as_ref())))
        {
            return Ok(());
        }

        // Take lock.
        let mut map = self.map.prepare_filesystem_sync()?;
        let mut dag = self.dag.prepare_filesystem_sync()?;

        // Build.
        build(
            &mut map,
            &mut dag,
            parent_names_func,
            master_names,
            non_master_names,
        )?;

        // Write to disk.
        map.sync()?;
        dag.sync(std::iter::once(&mut self.dag))?;
        Ok(())
    }

    /// Reload segments from disk.
    pub fn reload(&mut self) -> Result<()> {
        self.map.reload()?;
        self.dag.reload()?;
        Ok(())
    }

    // TODO: Consider implementing these:
    // - NamedSpanSet - SpanSet wrapper that only exposes "names".
    //   - Potentially, it has to implement smartset-like interfaces.
    // - On NameDag, methods wrapping dag algorithms that uses NamedSpanSet
    //   as input and output.
    // Before those APIs, LowLevelAccess might have to be used by callsites.
}

/// Export non-master DAG as parent_names_func on HashMap.
///
/// This can be expensive. It is expected to be either called infrequently,
/// or called with a small amount of data. For example, bounded amount of
/// non-master commits.
fn non_master_parent_names(
    map: &SyncableIdMap,
    dag: &SyncableIdDag,
) -> Result<HashMap<VertexName, Vec<VertexName>>> {
    let parent_ids = dag.non_master_parent_ids()?;
    // Map id to name.
    let parent_names = parent_ids
        .iter()
        .map(|(id, parent_ids)| {
            let name = map.vertex_name(*id)?;
            let parent_names = parent_ids
                .into_iter()
                .map(|p| map.vertex_name(*p))
                .collect::<Result<Vec<_>>>()?;
            Ok((name, parent_names))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    Ok(parent_names)
}

/// Re-assign ids and segments for non-master group.
pub fn rebuild_non_master(map: &mut SyncableIdMap, dag: &mut SyncableIdDag) -> Result<()> {
    // backup part of the named graph in memory.
    let parents = non_master_parent_names(map, dag)?;
    let mut heads = parents
        .keys()
        .collect::<HashSet<_>>()
        .difference(
            &parents
                .values()
                .flat_map(|ps| ps.into_iter())
                .collect::<HashSet<_>>(),
        )
        .map(|&v| v.clone())
        .collect::<Vec<_>>();
    heads.sort_unstable();

    // Remove existing non-master data.
    dag.remove_non_master()?;
    map.remove_non_master()?;

    // Rebuild them.
    let parent_func = |name: VertexName| match parents.get(&name) {
        Some(names) => Ok(names.iter().cloned().collect()),
        None => bail!(
            "bug: parents of {:?} is missing (in rebuild_non_master)",
            name
        ),
    };
    build(map, dag, parent_func, &[], &heads[..])?;

    Ok(())
}

/// Build IdMap and Segments for the given heads.
pub fn build<F>(
    map: &mut SyncableIdMap,
    dag: &mut SyncableIdDag,
    parent_names_func: F,
    master_heads: &[VertexName],
    non_master_heads: &[VertexName],
) -> Result<()>
where
    F: Fn(VertexName) -> Result<Vec<VertexName>>,
{
    // Update IdMap.
    for (nodes, group) in [
        (master_heads, Group::MASTER),
        (non_master_heads, Group::NON_MASTER),
    ]
    .iter()
    {
        for node in nodes.iter() {
            map.assign_head(node.clone(), &parent_names_func, *group)?;
        }
    }

    // Update segments.
    {
        let parent_ids_func = map.build_get_parents_by_id(&parent_names_func);
        for &group in Group::ALL.iter() {
            let id = map.next_free_id(group)?;
            if id > group.min_id() {
                dag.build_segments_persistent(id - 1, &parent_ids_func)?;
            }
        }
    }

    // Rebuild non-master ids and segments.
    if map.need_rebuild_non_master {
        rebuild_non_master(map, dag)?;
    }

    Ok(())
}

/// Provide low level access to dag and map.
/// Unsafe because it's possible to break consistency by writing to them.
///
/// This is currently used in Python bindings to provide an interface that is
/// consistent with `smartset.abstractsmartset`. Ideally, `smartset` provides
/// public commit hash interface, and there is no LowLevelAccess here.
pub unsafe trait LowLevelAccess {
    fn dag(&self) -> &IdDag;
    fn map(&self) -> &IdMap;
}

unsafe impl LowLevelAccess for NameDag {
    fn dag(&self) -> &IdDag {
        &self.dag
    }
    fn map(&self) -> &IdMap {
        &self.map
    }
}

fn is_ok_some<T>(value: Result<Option<T>>) -> bool {
    match value {
        Ok(Some(_)) => true,
        _ => false,
    }
}

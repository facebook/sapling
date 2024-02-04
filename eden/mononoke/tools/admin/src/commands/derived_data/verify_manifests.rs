/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Loadable;
use clap::builder::PossibleValuesParser;
use clap::Args;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_manager::BonsaiDerivable;
use fsnodes::RootFsnodeId;
use futures::future::try_join_all;
use futures::future::FutureExt;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_derivation::MappedHgChangesetId;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use skeleton_manifest::RootSkeletonManifestId;
use slog::trace;
use unodes::RootUnodeManifestId;

use super::Repo;

const MANIFEST_DERIVED_DATA_TYPES: &[&str] = &[
    RootFsnodeId::NAME,
    MappedHgChangesetId::NAME,
    RootUnodeManifestId::NAME,
    RootSkeletonManifestId::NAME,
];

#[derive(Args)]
pub(super) struct VerifyManifestsArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    /// Type of derived data representing a manifest
    #[clap(long, short = 'T', value_parser = PossibleValuesParser::new(MANIFEST_DERIVED_DATA_TYPES))]
    manifest_type: Vec<String>,
    /// Only verify the manifests if they are already derived
    #[clap(long)]
    if_derived: bool,
}

#[derive(Clone, Default)]
struct FileContentValue {
    values: Vec<ManifestData>,
}

impl fmt::Display for FileContentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, value) in self.values.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "({})", value)?;
        }
        Ok(())
    }
}

impl FileContentValue {
    pub fn new() -> Self {
        Self { values: vec![] }
    }

    pub fn update(&mut self, val: ManifestData) {
        self.values.push(val);
    }

    pub fn is_valid(&self, expected_manifests: &HashSet<ManifestType>) -> bool {
        if self.values.is_empty() {
            return false;
        }

        let manifest_types: HashSet<_> = self
            .values
            .iter()
            .map(ManifestData::manifest_type)
            .collect();
        if &manifest_types != expected_manifests {
            return false;
        }
        let contents: HashSet<_> = self
            .values
            .iter()
            .filter_map(ManifestData::content)
            .collect();
        // Skeleton manifests have no content, so 0 is valid for them.
        // Otherwise, we should have exactly one.
        contents.len() <= 1
    }
}

#[derive(Clone, Hash, Eq, PartialEq)]
enum ManifestType {
    Fsnodes,
    Hg,
    Unodes,
    Skeleton,
}

#[derive(Clone, Hash, Eq, PartialEq)]
enum ManifestData {
    Fsnodes(FileType, ContentId),
    Hg(FileType, ContentId),
    Unodes(FileType, ContentId),
    Skeleton,
}

impl fmt::Display for ManifestType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ManifestType::*;

        match &self {
            Fsnodes => write!(f, "Fsnodes"),
            Hg => write!(f, "Hg"),
            Unodes => write!(f, "Unodes"),
            Skeleton => write!(f, "Skeleton"),
        }
    }
}

impl ManifestData {
    fn manifest_type(&self) -> ManifestType {
        use ManifestData::*;

        match self {
            Fsnodes(..) => ManifestType::Fsnodes,
            Hg(..) => ManifestType::Hg,
            Unodes(..) => ManifestType::Unodes,
            Skeleton => ManifestType::Skeleton,
        }
    }

    fn content(&self) -> Option<(FileType, ContentId)> {
        use ManifestData::*;

        match self {
            Fsnodes(ty, id) | Hg(ty, id) | Unodes(ty, id) => Some((*ty, *id)),
            Skeleton => None,
        }
    }
}

impl fmt::Display for ManifestData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ManifestData::*;
        match &self {
            Fsnodes(ty, id) | Hg(ty, id) | Unodes(ty, id) => {
                write!(f, "{}: {}, {}", self.manifest_type(), ty, id)
            }
            Skeleton => write!(f, "{}: present", self.manifest_type()),
        }
    }
}

async fn derive_or_fetch<T: BonsaiDerived>(
    ctx: &CoreContext,
    repo: &Repo,
    csid: ChangesetId,
    fetch_derived: bool,
) -> Result<T> {
    if fetch_derived {
        let value = T::fetch_derived(ctx, repo, &csid).await?;
        value.ok_or_else(|| anyhow!("{} are not derived for {}", T::DERIVABLE_NAME, csid))
    } else {
        Ok(T::derive(ctx, repo, csid).await?)
    }
}

async fn list_hg_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
) -> Result<(ManifestType, HashMap<NonRootMPath, ManifestData>)> {
    let hg_cs_id = repo.derive_hg_changeset(ctx, cs_id).await?;

    let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
    let mfid = hg_cs.manifestid();

    let map: HashMap<_, _> = mfid
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(path, (ty, filenode_id))| async move {
            let filenode = filenode_id.load(ctx, repo.repo_blobstore()).await?;
            let content_id = filenode.content_id();
            let val = ManifestData::Hg(ty, content_id);
            Ok((path, val))
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await?;
    trace!(ctx.logger(), "Loaded hg manifests for {} paths", map.len());
    Ok((ManifestType::Hg, map))
}

async fn list_skeleton_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    fetch_derived: bool,
) -> Result<(ManifestType, HashMap<NonRootMPath, ManifestData>)> {
    let root_skeleton_id =
        derive_or_fetch::<RootSkeletonManifestId>(ctx, repo, cs_id, fetch_derived).await?;

    let skeleton_id = root_skeleton_id.skeleton_manifest_id();
    let map: HashMap<_, _> = skeleton_id
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(path, ())| (path, ManifestData::Skeleton))
        .try_collect()
        .await?;
    trace!(
        ctx.logger(),
        "Loaded skeleton manifests for {} paths",
        map.len()
    );
    Ok((ManifestType::Skeleton, map))
}

async fn list_fsnodes(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    fetch_derived: bool,
) -> Result<(ManifestType, HashMap<NonRootMPath, ManifestData>)> {
    let root_fsnode_id = derive_or_fetch::<RootFsnodeId>(ctx, repo, cs_id, fetch_derived).await?;

    let fsnode_id = root_fsnode_id.fsnode_id();
    let map: HashMap<_, _> = fsnode_id
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(path, fsnode)| {
            let (content_id, ty): (ContentId, FileType) = fsnode.into();
            let val = ManifestData::Fsnodes(ty, content_id);
            (path, val)
        })
        .try_collect()
        .await?;
    trace!(ctx.logger(), "Loaded fsnodes for {} paths", map.len());
    Ok((ManifestType::Fsnodes, map))
}

async fn list_unodes(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    fetch_derived: bool,
) -> Result<(ManifestType, HashMap<NonRootMPath, ManifestData>)> {
    let root_unode_id =
        derive_or_fetch::<RootUnodeManifestId>(ctx, repo, cs_id, fetch_derived).await?;

    let unode_id = root_unode_id.manifest_unode_id();
    let map: HashMap<_, _> = unode_id
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(path, unode_id)| async move {
            let unode = unode_id.load(ctx, repo.repo_blobstore()).await?;
            let val = ManifestData::Unodes(*unode.file_type(), *unode.content_id());
            Ok((path, val))
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await?;
    trace!(ctx.logger(), "Loaded unodes for {} paths", map.len());
    Ok((ManifestType::Unodes, map))
}

pub(super) async fn verify_manifests(
    ctx: &CoreContext,
    repo: &Repo,
    args: VerifyManifestsArgs,
) -> Result<()> {
    let cs_id = args
        .changeset_args
        .resolve_changeset(ctx, repo)
        .await?
        .ok_or_else(|| anyhow!("Changeset does not exist in this repository"))?;
    let fetch_derived = args.if_derived;
    let mut manifests = HashSet::new();
    let mut futs = vec![];
    for ty in args.manifest_type {
        if ty == RootFsnodeId::NAME {
            manifests.insert(ManifestType::Fsnodes);
            futs.push(list_fsnodes(ctx, repo, cs_id, fetch_derived).boxed());
        } else if ty == RootUnodeManifestId::NAME {
            manifests.insert(ManifestType::Unodes);
            futs.push(list_unodes(ctx, repo, cs_id, fetch_derived).boxed());
        } else if ty == MappedHgChangesetId::NAME {
            manifests.insert(ManifestType::Hg);
            futs.push(list_hg_manifest(ctx, repo, cs_id).boxed());
        } else if ty == RootSkeletonManifestId::NAME {
            manifests.insert(ManifestType::Skeleton);
            futs.push(list_skeleton_manifest(ctx, repo, cs_id, fetch_derived).boxed());
        } else {
            return Err(anyhow!("unknown derived data manifest type"));
        }
    }
    let mut combined: HashMap<NonRootMPath, FileContentValue> = HashMap::new();
    let contents = try_join_all(futs).await?;
    trace!(ctx.logger(), "Combining {} manifests", contents.len());
    for (mf_type, map) in contents {
        for (path, new_val) in map {
            combined
                .entry(path)
                .or_insert_with(FileContentValue::new)
                .update(new_val.clone());
        }
        trace!(ctx.logger(), "Completed {} manifest", mf_type);
    }

    trace!(ctx.logger(), "Checking {} paths", combined.len());
    let mut invalid_count = 0u64;
    for (path, val) in combined {
        if !val.is_valid(&manifests) {
            println!("Invalid!\nPath: {}", path);
            println!("{}\n", val);
            invalid_count += 1;
        }
    }
    if invalid_count == 0 {
        trace!(ctx.logger(), "Check complete");
    } else {
        trace!(ctx.logger(), "Found {} invalid paths", invalid_count);
    }

    Ok(())
}

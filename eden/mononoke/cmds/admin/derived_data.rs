/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{args, helpers::csid_resolve};
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_utils::{derived_data_utils, POSSIBLE_DERIVED_TYPES};
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{try_join_all, FutureExt as PreviewFutureExt},
    Future, TryStreamExt,
};
use manifest::ManifestOps;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::{ChangesetId, ContentId, FileType, MPath};
use slog::Logger;
use std::{
    collections::{HashMap, HashSet},
    fmt,
};
use unodes::RootUnodeManifestId;

use crate::error::SubcommandError;

pub const DERIVED_DATA: &str = "derived-data";
const SUBCOMMAND_EXISTS: &str = "exists";
const SUBCOMMAND_VERIFY_MANIFESTS: &str = "verify-manifests";

const ARG_HASH_OR_BOOKMARK: &str = "hash-or-bookmark";
const ARG_TYPE: &str = "type";

const MANIFEST_DERIVED_DATA_TYPES: &'static [&'static str] = &[
    RootFsnodeId::NAME,
    MappedHgChangesetId::NAME,
    RootUnodeManifestId::NAME,
];

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(DERIVED_DATA)
        .about("request information about derived data")
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_EXISTS)
                .about("check if derived data has been generated")
                .arg(
                    Arg::with_name(ARG_TYPE)
                        .help("type of derived data")
                        .takes_value(true)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_HASH_OR_BOOKMARK)
                        .help("(hg|bonsai) commit hash or bookmark")
                        .takes_value(true)
                        .multiple(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_VERIFY_MANIFESTS)
                .about("compare check if derived data has been generated")
                .arg(
                    Arg::with_name(ARG_TYPE)
                        .help("types of derived data representing a manifest")
                        .long(ARG_TYPE)
                        .takes_value(true)
                        .multiple(true)
                        .possible_values(MANIFEST_DERIVED_DATA_TYPES),
                )
                .arg(
                    Arg::with_name(ARG_HASH_OR_BOOKMARK)
                        .help("(hg|bonsai) commit hash or bookmark")
                        .takes_value(true)
                        .required(true),
                ),
        )
}

pub fn subcommand_derived_data(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Pin<Box<dyn Future<Output = Result<(), SubcommandError>> + Send>> {
    args::init_cachelib(fb, &matches, None);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, &logger, &matches);

    match sub_m.subcommand() {
        (SUBCOMMAND_EXISTS, Some(arg_matches)) => {
            let hashes_or_bookmarks: Vec<_> = arg_matches
                .values_of(ARG_HASH_OR_BOOKMARK)
                .map(|matches| matches.map(|cs| cs.to_string()).collect())
                .unwrap();

            let derived_data_type = arg_matches
                .value_of(ARG_TYPE)
                .map(|m| m.to_string())
                .unwrap();

            async move {
                let repo = repo.compat().await?;
                check_derived_data_exists(ctx, repo, derived_data_type, hashes_or_bookmarks).await
            }
            .boxed()
        }
        (SUBCOMMAND_VERIFY_MANIFESTS, Some(arg_matches)) => {
            let hash_or_bookmark = arg_matches
                .value_of(ARG_HASH_OR_BOOKMARK)
                .map(|m| m.to_string())
                .unwrap();

            let derived_data_types = arg_matches
                .values_of(ARG_TYPE)
                .map(|matches| matches.map(|cs| cs.to_string()).collect())
                .unwrap_or_else(|| {
                    MANIFEST_DERIVED_DATA_TYPES
                        .into_iter()
                        .map(|s| String::from(*s))
                        .collect::<Vec<_>>()
                });

            async move {
                let repo = repo.compat().await?;
                verify_manifests(ctx, repo, derived_data_types, hash_or_bookmark).await
            }
            .boxed()
        }
        _ => async move { Err(SubcommandError::InvalidArgs) }.boxed(),
    }
}

async fn check_derived_data_exists(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_data_type: String,
    hashes_or_bookmarks: Vec<String>,
) -> Result<(), SubcommandError> {
    let derived_utils = derived_data_utils(repo.clone(), derived_data_type)?;

    let cs_id_futs: Vec<_> = hashes_or_bookmarks
        .into_iter()
        .map(|hash_or_bm| csid_resolve(ctx.clone(), repo.clone(), hash_or_bm).compat())
        .collect();

    let cs_ids = try_join_all(cs_id_futs).await?;

    let pending = derived_utils
        .pending(ctx.clone(), repo.clone(), cs_ids.clone())
        .compat()
        .await?;

    for cs_id in cs_ids {
        if pending.contains(&cs_id) {
            println!("Not Derived: {}", cs_id);
        } else {
            println!("Derived: {}", cs_id);
        }
    }

    Ok(())
}

async fn verify_manifests(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_data_types: Vec<String>,
    hash_or_bookmark: String,
) -> Result<(), SubcommandError> {
    let cs_id = csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
        .compat()
        .await?;
    let mut manifests = HashSet::new();
    let mut futs = vec![];
    for ty in derived_data_types {
        if ty == RootFsnodeId::NAME {
            manifests.insert(ManifestType::Fsnodes);
            futs.push(list_fsnodes(&ctx, &repo, cs_id).boxed());
        } else if ty == RootUnodeManifestId::NAME {
            manifests.insert(ManifestType::Unodes);
            futs.push(list_unodes(&ctx, &repo, cs_id).boxed());
        } else if ty == MappedHgChangesetId::NAME {
            manifests.insert(ManifestType::Hg);
            futs.push(list_hg_manifest(&ctx, &repo, cs_id).boxed());
        } else {
            return Err(anyhow!("unknown derived data manifest type").into());
        }
    }
    let mut combined: HashMap<MPath, FileContentValue> = HashMap::new();
    let contents = try_join_all(futs).await?;
    for map in contents {
        for (path, new_val) in map {
            combined
                .entry(path)
                .or_insert_with(FileContentValue::new)
                .update(new_val.clone());
        }
    }

    for (path, val) in combined {
        if !val.is_valid(&manifests) {
            println!("Invalid!\nPath: {}", path);
            println!("{}\n", val);
        }
    }

    Ok(())
}

#[derive(Clone, Default)]
struct FileContentValue {
    values: Vec<(FileType, ContentId, ManifestType)>,
}

impl fmt::Display for FileContentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for value in &self.values {
            write!(f, "{}: {}, {})", value.2, value.0, value.1)?;
        }
        Ok(())
    }
}

impl FileContentValue {
    pub fn new() -> Self {
        Self { values: vec![] }
    }

    pub fn update(&mut self, val: (FileType, ContentId, ManifestType)) {
        self.values.push(val);
    }

    pub fn is_valid(&self, expected_manifests: &HashSet<ManifestType>) -> bool {
        if self.values.is_empty() {
            return false;
        }

        let manifest_types: HashSet<_> = self.values.iter().map(|item| &item.2).cloned().collect();
        if &manifest_types != expected_manifests {
            return false;
        }
        let first = &self.values[0];
        self.values
            .iter()
            .all(|item| first.0 == item.0 && first.1 == item.1)
    }
}

#[derive(Clone, Hash, Eq, PartialEq)]
enum ManifestType {
    Fsnodes,
    Hg,
    Unodes,
}

impl fmt::Display for ManifestType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ManifestType::*;

        match &self {
            Fsnodes => write!(f, "Fsnodes"),
            Hg => write!(f, "Hg"),
            Unodes => write!(f, "Unodes"),
        }
    }
}

async fn list_hg_manifest(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, (FileType, ContentId, ManifestType)>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;

    let hg_cs = hg_cs_id.load(ctx.clone(), repo.blobstore()).await?;
    let mfid = hg_cs.manifestid();

    mfid.list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .compat()
        .map_ok(|(path, (ty, filenode_id))| async move {
            let filenode = filenode_id.load(ctx.clone(), repo.blobstore()).await?;
            let content_id = filenode.content_id();
            let val = (ty, content_id, ManifestType::Hg);
            Ok((path, val))
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

async fn list_fsnodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, (FileType, ContentId, ManifestType)>, Error> {
    let root_fsnode_id = RootFsnodeId::derive(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;

    let fsnode_id = root_fsnode_id.fsnode_id();
    fsnode_id
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .compat()
        .map_ok(|(path, (content_id, ty))| {
            let val = (ty, content_id, ManifestType::Fsnodes);
            (path, val)
        })
        .try_collect()
        .await
}

async fn list_unodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, (FileType, ContentId, ManifestType)>, Error> {
    let root_unode_id = RootUnodeManifestId::derive(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;

    let unode_id = root_unode_id.manifest_unode_id();
    unode_id
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .compat()
        .map_ok(|(path, unode_id)| async move {
            let unode = unode_id.load(ctx.clone(), repo.blobstore()).await?;
            let val = (
                *unode.file_type(),
                *unode.content_id(),
                ManifestType::Unodes,
            );
            Ok((path, val))
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

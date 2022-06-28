/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use filenodes::FilenodeInfo;
use filenodes::FilenodeResult;
use filenodes::PreparedFilenode;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::NULL_HASH;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::RepoPath;
use std::collections::HashMap;

use crate::derive::derive_filenodes;
use crate::derive::derive_filenodes_in_batch;

use derived_data_service_if::types as thrift;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRootFilenode {
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl From<PreparedRootFilenode> for PreparedFilenode {
    fn from(root_filenode: PreparedRootFilenode) -> Self {
        let PreparedRootFilenode {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = root_filenode;

        PreparedFilenode {
            path: RepoPath::RootPath,
            info: FilenodeInfo {
                filenode,
                p1,
                p2,
                copyfrom,
                linknode,
            },
        }
    }
}

impl TryFrom<PreparedFilenode> for PreparedRootFilenode {
    type Error = Error;

    fn try_from(filenode: PreparedFilenode) -> Result<Self> {
        let PreparedFilenode { path, info } = filenode;

        let FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = info;

        if path != RepoPath::RootPath {
            return Err(format_err!("unexpected path for root filenode: {:?}", path));
        }
        Ok(Self {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    }
}

/// Derives filenodes that are stores in Filenodes object (usually in a database).
/// Note: that should be derived only for public commits!
///
/// Filenodes might be disabled, in that case FilenodesOnlyPublic will always return
/// FilenodesOnlyPublic::Disabled enum variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilenodesOnlyPublic {
    Present {
        root_filenode: Option<PreparedRootFilenode>,
    },
    Disabled,
}

#[async_trait]
impl BonsaiDerivable for FilenodesOnlyPublic {
    const NAME: &'static str = "filenodes";

    type Dependencies = dependencies![MappedHgChangesetId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self> {
        derive_filenodes(ctx, derivation_ctx, bonsai).await
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let filenodes = derivation_ctx.filenodes()?;
        let prepared = derive_filenodes_in_batch(ctx, derivation_ctx, bonsais).await?;
        let mut res = HashMap::with_capacity(prepared.len());
        for (cs_id, public_filenode, non_roots) in prepared.into_iter() {
            let filenode = match public_filenode {
                FilenodesOnlyPublic::Present { root_filenode } => match root_filenode {
                    Some(filenode) if !non_roots.is_empty() => {
                        match filenodes.add_filenodes(ctx, non_roots).await? {
                            FilenodeResult::Disabled => FilenodesOnlyPublic::Disabled,
                            FilenodeResult::Present(()) => FilenodesOnlyPublic::Present {
                                root_filenode: Some(filenode),
                            },
                        }
                    }
                    _ => FilenodesOnlyPublic::Present { root_filenode },
                },
                FilenodesOnlyPublic::Disabled => FilenodesOnlyPublic::Disabled,
            };
            res.insert(cs_id, filenode.clone());
        }
        Ok(res)
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        _changeset_id: ChangesetId,
    ) -> Result<()> {
        let root_filenode = match self {
            FilenodesOnlyPublic::Present { root_filenode } => match root_filenode {
                Some(root_filenode) => root_filenode,
                None => return Ok(()),
            },
            FilenodesOnlyPublic::Disabled => return Ok(()),
        };

        match derivation_ctx
            .filenodes()?
            .add_filenodes(ctx, vec![root_filenode.into()])
            .await?
        {
            FilenodeResult::Present(()) => Ok(()),
            FilenodeResult::Disabled => {
                // Filenodes got disabled just after we finished deriving them
                // but before we stored the mapping.  Ideally we would return
                // FilenodesMaybePublic::Disabled to the caller, but in this
                // very small window there is no way to do that. Instead we
                // must fail the request.
                bail!("filenodes were disabled after being successfully derived")
            }
        }
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        if tunables::tunables().get_filenodes_disabled() {
            return Ok(Some(FilenodesOnlyPublic::Disabled));
        }
        let filenode_res = fetch_root_filenode(ctx, derivation_ctx, changeset_id).await?;
        let maybe_root_filenode = match filenode_res {
            FilenodeResult::Present(maybe_root_filenode) => maybe_root_filenode,
            FilenodeResult::Disabled => {
                return Ok(Some(FilenodesOnlyPublic::Disabled));
            }
        };

        Ok(
            maybe_root_filenode.map(move |filenode| FilenodesOnlyPublic::Present {
                root_filenode: Some(filenode),
            }),
        )
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        match data {
            thrift::DerivedData::filenode(thrift::DerivedDataFilenode::filenode_present(
                filenode,
            )) => match filenode.root_filenode {
                None => Ok(FilenodesOnlyPublic::Present {
                    root_filenode: None,
                }),
                Some(data) => Ok(FilenodesOnlyPublic::Present {
                    root_filenode: Some(
                        FilenodeInfo::from_thrift(data)
                            .map(|info| PreparedFilenode {
                                path: RepoPath::RootPath,
                                info,
                            })?
                            .try_into()?,
                    ),
                }),
            },
            thrift::DerivedData::filenode(thrift::DerivedDataFilenode::filenode_disabled(_)) => {
                Ok(FilenodesOnlyPublic::Disabled)
            }
            _ => Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            )),
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        match data {
            FilenodesOnlyPublic::Present {
                root_filenode: Some(root),
            } => Ok(thrift::DerivedData::filenode(
                thrift::DerivedDataFilenode::filenode_present(thrift::DerivedDataFilenodePresent {
                    root_filenode: Some(PreparedFilenode::from(root).info.into_thrift()),
                }),
            )),
            FilenodesOnlyPublic::Present {
                root_filenode: None,
            } => Ok(thrift::DerivedData::filenode(
                thrift::DerivedDataFilenode::filenode_present(thrift::DerivedDataFilenodePresent {
                    root_filenode: None,
                }),
            )),
            FilenodesOnlyPublic::Disabled => Ok(thrift::DerivedData::filenode(
                thrift::DerivedDataFilenode::filenode_disabled(thrift::DisabledFilenodes {}),
            )),
        }
    }
}

async fn fetch_root_filenode(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
) -> Result<FilenodeResult<Option<PreparedRootFilenode>>> {
    // If hg changeset is not generated, then root filenode can't possible be generated
    // Check it and return None if hg changeset is not generated
    let maybe_hg_cs_id = derivation_ctx
        .bonsai_hg_mapping()?
        .get_hg_from_bonsai(ctx, cs_id)
        .await?;
    let hg_cs_id = if let Some(hg_cs_id) = maybe_hg_cs_id {
        hg_cs_id
    } else {
        return Ok(FilenodeResult::Present(None));
    };

    let mf_id = hg_cs_id
        .load(ctx, &derivation_ctx.blobstore())
        .await?
        .manifestid();

    // Special case null manifest id if we run into it
    let mf_id = mf_id.into_nodehash();
    if mf_id == NULL_HASH {
        Ok(FilenodeResult::Present(Some(PreparedRootFilenode {
            filenode: HgFileNodeId::new(NULL_HASH),
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: HgChangesetId::new(NULL_HASH),
        })))
    } else {
        let filenode_res = derivation_ctx
            .filenodes()?
            .get_filenode(ctx, &RepoPath::RootPath, HgFileNodeId::new(mf_id))
            .await?;

        match filenode_res {
            FilenodeResult::Present(maybe_info) => {
                let info = maybe_info
                    .map(|info| {
                        PreparedRootFilenode::try_from(PreparedFilenode {
                            path: RepoPath::RootPath,
                            info,
                        })
                    })
                    .transpose()?;
                Ok(FilenodeResult::Present(info))
            }
            FilenodeResult::Disabled => Ok(FilenodeResult::Disabled),
        }
    }
}

impl_bonsai_derived_via_manager!(FilenodesOnlyPublic);

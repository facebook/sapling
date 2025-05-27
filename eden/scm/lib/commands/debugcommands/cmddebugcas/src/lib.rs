/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use async_runtime::try_block_unless_interrupted as block_on;
use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::Result;
use cmdutil::define_flags;
use futures::TryStreamExt;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use manifest_augmented_tree::AugmentedTree;
use repo::Repo;
use types::CasDigest;
use types::CasDigestType;
use types::FetchContext;
use types::RepoPath;
use workingcopy::WorkingCopy;

define_flags! {
    pub struct DebugCasOpts {
        /// Revision
        #[short('r')]
        #[argtype("REV")]
        rev: Option<String>,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugCasOpts>, repo: &Repo, wc: &WorkingCopy) -> Result<u8> {
    let client = match cas_client::new(ctx.config().clone())? {
        Some(client) => client,
        None => abort!("no CAS client constructor registered"),
    };

    let commit = repo.resolve_commit(
        Some(&wc.treestate().lock()),
        ctx.opts.rev.as_deref().unwrap_or("."),
    )?;
    let manifest = repo.tree_resolver()?.get(&commit)?;

    let mut output = ctx.io().output();

    for path in &ctx.opts.args {
        let path = RepoPath::from_str(path)?;
        match manifest.get(path)? {
            None => abort!("path {path} not in manifest"),
            Some(FsNodeMetadata::Directory(hgid)) => {
                let hgid = hgid.unwrap();
                let aux =
                    repo.tree_store()?
                        .get_tree_aux_data(FetchContext::default(), path, hgid)?;

                let fetch_res = block_on(async {
                    client
                        .fetch(
                            FetchContext::default(),
                            &[CasDigest {
                                hash: aux.augmented_manifest_id,
                                size: aux.augmented_manifest_size,
                            }],
                            CasDigestType::Tree,
                        )
                        .await
                        .map_ok(|(_, data)| data)
                        .try_collect::<Vec<_>>()
                        .await
                })?;

                for (digest, res) in fetch_res.into_iter().flatten() {
                    write!(output, "tree path {path}, node {hgid}, digest {digest:?}, ")?;

                    match res {
                        Ok(Some(contents)) => {
                            let aug_tree =
                                AugmentedTree::try_deserialize(contents.into_bytes().as_ref())?;
                            write!(output, "contents:\n{aug_tree:#?}\n\n",)?
                        }
                        Ok(None) => write!(output, "not found in CAS\n\n",)?,
                        Err(err) => write!(output, "error: {err:?}\n")?,
                    }
                }
            }
            Some(FsNodeMetadata::File(FileMetadata { hgid, .. })) => {
                let aux = repo
                    .file_store()?
                    .get_aux(FetchContext::default(), path, hgid)?;

                let fetch_res = block_on(async {
                    client
                        .fetch(
                            FetchContext::default(),
                            &[CasDigest {
                                hash: aux.blake3,
                                size: aux.total_size,
                            }],
                            CasDigestType::File,
                        )
                        .await
                        .map_ok(|(_, data)| data)
                        .try_collect::<Vec<_>>()
                        .await
                })?;

                for (digest, res) in fetch_res.into_iter().flatten() {
                    write!(output, "file path {path}, node {hgid}, digest {digest:?}, ")?;

                    match res {
                        Ok(Some(contents)) => write!(
                            output,
                            "contents:\n{}\n\n",
                            util::utf8::escape_non_utf8(contents.into_bytes().as_ref())
                        )?,
                        Ok(None) => write!(output, "not found in CAS\n\n",)?,
                        Err(err) => write!(output, "error: {err:?}\n")?,
                    }
                }
            }
        }
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugcas"
}

pub fn doc() -> &'static str {
    "debug CAS queries"
}

pub fn synopsis() -> Option<&'static str> {
    None
}

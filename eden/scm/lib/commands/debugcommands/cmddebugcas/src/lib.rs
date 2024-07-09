/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use async_runtime::try_block_unless_interrupted as block_on;
use clidispatch::abort;
use clidispatch::ReqCtx;
use cmdutil::define_flags;
use cmdutil::Result;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use repo::Repo;
use types::fetch_mode::FetchMode;
use types::CasDigest;
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

pub fn run(ctx: ReqCtx<DebugCasOpts>, repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    let client = match cas_client::new(ctx.config())? {
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
            Some(FsNodeMetadata::Directory(_hgid)) => {
                abort!("directories not supported yet");
            }
            Some(FsNodeMetadata::File(FileMetadata { hgid, .. })) => {
                let aux = repo
                    .file_store()?
                    .get_aux(path, hgid, FetchMode::AllowRemote)?;
                let fetch_res = block_on(client.fetch(&[CasDigest {
                    hash: aux.blake3,
                    size: aux.total_size,
                }]))?;
                for (digest, res) in fetch_res {
                    write!(output, "path {path}, node {hgid}, digest {digest:?}, ")?;

                    match res {
                        Ok(contents) => write!(
                            output,
                            "contents:\n{}\n\n",
                            util::utf8::escape_non_utf8(&contents)
                        )?,
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

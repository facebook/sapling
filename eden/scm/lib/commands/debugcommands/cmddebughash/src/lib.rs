/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use cmdutil::Result as CmdResult;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use manifest::FsNodeMetadata;
use manifest::List;
use manifest::Manifest;
use pathmatcher::DirectoryMatch;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::Matcher;
use rayon::prelude::*;
use repo::CoreRepo;
use sha1::Digest;
use sha1::Sha1;
use types::HgId;
use types::RepoPathBuf;
use types::hgid::NULL_ID;

define_flags! {
    pub struct DebugHashOpts {
        walk_opts: WalkOpts,

        /// compute hash at REV
        #[short('r')]
        #[argtype("REV")]
        rev: Option<String>,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugHashOpts>, repo: &CoreRepo) -> CmdResult<u8> {
    abort_if!(ctx.opts.args.is_empty(), "PATTERN required");

    let (repo_root, case_sensitive, cwd, patterns, rev) = match repo {
        CoreRepo::Disk(repo) => {
            let wc = repo.working_copy()?;
            let wc = wc.read();
            let vfs = wc.vfs();
            (
                Some(vfs.root().to_path_buf()),
                vfs.case_sensitive(),
                std::env::current_dir()?,
                ctx.opts.args.clone(),
                ctx.opts.rev.as_deref().unwrap_or("wdir"),
            )
        }
        CoreRepo::Slapi(_slapi_repo) => (
            None,
            true,
            PathBuf::new(),
            ctx.opts.args.clone(),
            match ctx.opts.rev.as_deref() {
                Some(rev) => rev,
                None => abort!("--rev is required for repoless debughash"),
            },
        ),
    };

    let cli_matcher_root = repo_root.as_deref().unwrap_or(Path::new(""));
    let hinted_matcher = pathmatcher::cli_matcher(
        &patterns,
        &ctx.opts.walk_opts.include,
        &ctx.opts.walk_opts.exclude,
        pathmatcher::PatternKind::RelPath,
        case_sensitive,
        cli_matcher_root,
        &cwd,
        &mut ctx.io().input(),
    )?;
    let matcher: DynMatcher = Arc::new(hinted_matcher);

    let (_, manifest) = repo.resolve_manifest(&ctx.core, rev, matcher.clone())?;

    let matcher = if let Some(sparse_matcher) = repo.sparse_matcher(&manifest)? {
        Arc::new(IntersectMatcher::new(vec![matcher, sparse_matcher])) as DynMatcher
    } else {
        matcher
    };

    let res = compute_hash(&manifest, RepoPathBuf::new(), &*matcher)?;
    let hash = res.map(|(_, hgid)| hgid).unwrap_or(NULL_ID);

    let mut out = ctx.io().output();
    write!(out, "{}\n", hash.to_hex())?;

    Ok(0)
}

/// Recursively compute a hash over matching manifest entries.
///
/// At each directory:
/// - `Everything`: use the directory's tree hash directly (avoids recursion)
/// - `ShouldTraverse`: recurse into the directory
/// - `Nothing`: skip entirely
///
/// At each level, collected HgIds are sorted and SHA1'd together.
fn compute_hash(
    manifest: &(impl Manifest + Sync),
    path: RepoPathBuf,
    matcher: &(dyn Matcher + Sync),
) -> Result<Option<(RepoPathBuf, HgId)>> {
    let entries = match manifest.list(&path)? {
        List::NotFound => return Ok(None),
        List::File => return Ok(None),
        List::Directory(entries) => entries,
    };

    let mut entries_to_hash: Vec<(RepoPathBuf, HgId)> = Vec::new();
    let mut to_recurse: Vec<RepoPathBuf> = Vec::new();
    let mut child_path = path.to_owned();

    for (name, metadata) in entries {
        child_path.push(&name);

        match metadata {
            FsNodeMetadata::File(file_meta) => {
                if matcher.matches_file(&child_path)? {
                    entries_to_hash.push((child_path.clone(), file_meta.hgid));
                }
            }
            FsNodeMetadata::Directory(dir_hgid) => match matcher.matches_directory(&child_path)? {
                DirectoryMatch::Everything => {
                    if let Some(hgid) = dir_hgid {
                        entries_to_hash.push((child_path.clone(), hgid));
                    } else {
                        to_recurse.push(child_path.clone());
                    }
                }
                DirectoryMatch::ShouldTraverse => {
                    to_recurse.push(child_path.clone());
                }
                DirectoryMatch::Nothing => {}
            },
        }

        child_path.pop();
    }

    // Recurse into subdirectories in parallel.
    let sub_results: Vec<Result<Option<(RepoPathBuf, HgId)>>> = to_recurse
        .into_par_iter()
        .map(|child_path| compute_hash(manifest, child_path, matcher))
        .collect();

    for result in sub_results {
        if let Some(h) = result? {
            entries_to_hash.push(h);
        }
    }

    if entries_to_hash.is_empty() {
        return Ok(None);
    }

    entries_to_hash.sort();

    let mut hasher = Sha1::new();
    for e in &entries_to_hash {
        hasher.update(e.0.as_byte_slice());
        hasher.update(e.1.as_ref());
    }
    let result: [u8; 20] = hasher.finalize().into();

    Ok(Some((path, HgId::from_byte_array(result))))
}

pub fn aliases() -> &'static str {
    "debughash"
}

pub fn doc() -> &'static str {
    r#"compute a recursive content hash over matching files

    Compute a single hash over the matched files in the manifest. The hash is
    computed by walking the manifest tree and hashing file node hashes together.
    Exclusions cause affected subtrees to be "exploded" so the hash is only
    sensitive to included files. When an entire subtree matches, the tree's own
    hash is used directly for efficiency.

    Use ``-r/--rev REV`` to hash files at a specific revision. Defaults
    to "wdir", which includes uncommitted changes.

    Use ``-X`` to exclude paths from the hash. This allows computing a
    hash that is not sensitive to changes in excluded files.

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[-r REV] PATTERN...")
}

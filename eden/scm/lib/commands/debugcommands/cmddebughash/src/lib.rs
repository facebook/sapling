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
use repo::CoreRepo;
use sha1::Digest;
use sha1::Sha1;
use slex::FoldScope;
use slex::Work;
use slex::WorkOptions;
use types::HgId;
use types::RepoPathBuf;
use types::hgid::NULL_ID;
use types::hgid::WDIR_ID;

define_flags! {
    pub struct DebugHashOpts {
        walk_opts: WalkOpts,

        /// compute hash at REV
        #[short('r')]
        #[argtype("REV")]
        rev: Option<String>,

        /// include unknown (untracked) files in the hash (wdir only)
        unknown: bool,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugHashOpts>, repo: &CoreRepo) -> CmdResult<u8> {
    abort_if!(ctx.opts.args.is_empty(), "PATTERN required");

    let include_unknown = ctx.opts.unknown;

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
        CoreRepo::Slapi(_slapi_repo) => {
            abort_if!(
                include_unknown,
                "--unknown is not supported for repoless debughash"
            );
            (
                None,
                true,
                PathBuf::new(),
                ctx.opts.args.clone(),
                match ctx.opts.rev.as_deref() {
                    Some(rev) => rev,
                    None => abort!("--rev is required for repoless debughash"),
                },
            )
        }
    };

    abort_if!(
        include_unknown && rev != "wdir",
        "--unknown is only supported for the working directory (wdir)"
    );

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

    let (_, manifest) = if include_unknown {
        // --unknown requires direct access to working_manifest with include_unknown=true.
        match repo {
            CoreRepo::Disk(repo) => {
                let wc = repo.working_copy()?;
                let wc = wc.read();
                let manifest = wc.working_manifest(&ctx.core, matcher.clone(), true)?;
                (WDIR_ID, manifest)
            }
            _ => unreachable!(),
        }
    } else {
        repo.resolve_manifest(&ctx.core, rev, matcher.clone())?
    };

    let matcher = if let Some(sparse_matcher) = repo.sparse_matcher(&manifest)? {
        Arc::new(IntersectMatcher::new(vec![matcher, sparse_matcher])) as DynMatcher
    } else {
        matcher
    };

    let hash = compute_hash(Arc::new(manifest), matcher)?;

    let mut out = ctx.io().output();
    write!(out, "{}\n", hash.to_hex())?;

    Ok(0)
}

#[derive(Debug)]
struct HashWork {
    path: RepoPathBuf,
    subtree_matches_everything: bool,
}

type HashEntry = (RepoPathBuf, HgId);
type FoldHashEntry = Option<HashEntry>;

/// Compute a hash over matching manifest entries.
///
/// Fully-matched durable directories collapse to their tree hash. Other directories are folded
/// postorder, so only active DFS lanes and unreduced ancestors stay in memory.
fn compute_hash(
    manifest: Arc<impl Manifest + Send + Sync + 'static>,
    matcher: DynMatcher,
) -> Result<HgId> {
    let root = HashWork {
        path: RepoPathBuf::new(),
        subtree_matches_everything: false,
    };
    let hgid = Work::fold_tree(
        WorkOptions::new(),
        root,
        move |work, scope| list_hash_work(&*manifest, &*matcher, work, scope),
        |work, entries| Ok(hash_directory_entries(work.path.clone(), entries)),
    )?;
    Ok(hgid.map(|(_, hgid)| hgid).unwrap_or(NULL_ID))
}

fn list_hash_work(
    manifest: &(impl Manifest + Sync),
    matcher: &(dyn Matcher + Sync),
    work: &HashWork,
    scope: &mut FoldScope<HashWork, FoldHashEntry>,
) -> Result<()> {
    let entries = match manifest.list(&work.path)? {
        List::NotFound | List::File => return Ok(()),
        List::Directory(entries) => entries,
    };

    let mut child_path = work.path.clone();
    for (name, metadata) in entries {
        child_path.push(&name);
        match metadata {
            FsNodeMetadata::File(file_meta) => {
                if work.subtree_matches_everything || matcher.matches_file(&child_path)? {
                    scope.resolve_child(Some((child_path.clone(), file_meta.hgid)));
                }
            }
            FsNodeMetadata::Directory(hgid) => {
                if work.subtree_matches_everything {
                    add_directory_hash_work(scope, child_path.clone(), hgid, true);
                } else {
                    match matcher.matches_directory(&child_path)? {
                        DirectoryMatch::Nothing => {}
                        DirectoryMatch::ShouldTraverse => {
                            add_directory_hash_work(scope, child_path.clone(), hgid, false);
                        }
                        DirectoryMatch::Everything => {
                            add_directory_hash_work(scope, child_path.clone(), hgid, true);
                        }
                    }
                }
            }
        }
        child_path.pop();
    }

    Ok(())
}

fn add_directory_hash_work(
    scope: &mut FoldScope<HashWork, FoldHashEntry>,
    path: RepoPathBuf,
    hgid: Option<HgId>,
    subtree_matches_everything: bool,
) {
    match hgid {
        Some(hgid) if subtree_matches_everything => scope.resolve_child(Some((path, hgid))),
        _ => scope.submit_child(HashWork {
            path,
            subtree_matches_everything,
        }),
    }
}

fn hash_directory_entries(path: RepoPathBuf, entries_to_hash: Vec<FoldHashEntry>) -> FoldHashEntry {
    let mut entries_to_hash: Vec<_> = entries_to_hash.into_iter().flatten().collect();
    if entries_to_hash.is_empty() {
        return None;
    }

    entries_to_hash.sort();

    let mut hasher = Sha1::new();
    for e in &entries_to_hash {
        hasher.update(e.0.as_byte_slice());
        hasher.update(e.1.as_ref());
    }
    let result: [u8; 20] = hasher.finalize().into();

    Some((path, HgId::from_byte_array(result)))
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

    Use ``--unknown`` to include unknown (untracked) files in the hash.
    This is only supported for the working directory (wdir).

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[--unknown] [-r REV] PATTERN...")
}

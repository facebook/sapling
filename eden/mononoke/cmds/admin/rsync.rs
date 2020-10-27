/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::save_bonsai_changesets;
use clap::{App, Arg, ArgMatches, SubCommand};
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::{compat::Future01CompatExt, future::try_join, TryStreamExt};

use blobrepo::BlobRepo;
use cmdlib::{args, helpers};
use context::CoreContext;
use manifest::{Entry, ManifestOps};
use mononoke_types::{
    fsnode::FsnodeFile, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange,
    MPath,
};
use regex::Regex;
use slog::{debug, info, Logger};
use std::{collections::BTreeMap, num::NonZeroU64};

use crate::error::SubcommandError;

pub const ARG_COMMIT_AUTHOR: &str = "commit-author";
pub const ARG_COMMIT_MESSAGE: &str = "commit-message";
pub const ARG_CSID: &str = "csid";
pub const ARG_EXCLUDE_FILE_REGEX: &str = "exclude-file-regex";
pub const ARG_FILE_NUM_LIMIT: &str = "file-num-limit";
pub const ARG_TOTAL_SIZE_LIMIT: &str = "total-size-limit";
pub const ARG_FROM_DIR: &str = "from-dir";
pub const ARG_LFS_THRESHOLD: &str = "lfs-threshold";
pub const ARG_OVERWRITE: &str = "overwrite";
pub const ARG_TO_DIR: &str = "to-dir";
pub const RSYNC: &str = "rsync";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(RSYNC)
        .about("creates commits that copy content of one directory into another")
        .arg(
            Arg::with_name(ARG_CSID)
                .long(ARG_CSID)
                .takes_value(true)
                .required(true)
                .help("{hg|bonsai} changeset id or bookmark name"),
        )
        .arg(
            Arg::with_name(ARG_FROM_DIR)
                .long(ARG_FROM_DIR)
                .takes_value(true)
                .required(true)
                .help(
                    "name of the directory to copy from. \
                       Error is return if this path doesn't exist or if it's a file",
                ),
        )
        .arg(
            Arg::with_name(ARG_TO_DIR)
                .long(ARG_TO_DIR)
                .takes_value(true)
                .required(true)
                .help(
                    "name of the directory to copy to. \
                       Error is return if this path is a file",
                ),
        )
        .arg(
            Arg::with_name(ARG_COMMIT_MESSAGE)
                .long(ARG_COMMIT_MESSAGE)
                .help("commit message to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_COMMIT_AUTHOR)
                .long(ARG_COMMIT_AUTHOR)
                .help("commit author to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_FILE_NUM_LIMIT)
                .long(ARG_FILE_NUM_LIMIT)
                .help("limit the number of files moved in a commit")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_EXCLUDE_FILE_REGEX)
                .long(ARG_EXCLUDE_FILE_REGEX)
                .help("exclude files that should not be copied")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_TOTAL_SIZE_LIMIT)
                .long(ARG_TOTAL_SIZE_LIMIT)
                .help("total size of all files in a commit")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_LFS_THRESHOLD)
                .long(ARG_LFS_THRESHOLD)
                .help(
                    "lfs threshold - files with size above that are excluded from file size limit",
                )
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_OVERWRITE)
                .long(ARG_OVERWRITE)
                .help("overwrite a file if it exists in the destination directory")
                .takes_value(false)
                .required(false),
        )
}

pub async fn subcommand_rsync<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    args::init_cachelib(fb, &matches, None);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, &logger, &matches).compat().await?;

    let cs_id = sub_matches
        .value_of(ARG_CSID)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_CSID))?;

    let cs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;

    let from_dir = sub_matches
        .value_of(ARG_FROM_DIR)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_FROM_DIR))?;
    let from_dir = MPath::new(from_dir)?;

    let to_dir = sub_matches
        .value_of(ARG_TO_DIR)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_TO_DIR))?;
    let to_dir = MPath::new(to_dir)?;

    let author = sub_matches
        .value_of(ARG_COMMIT_AUTHOR)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_COMMIT_AUTHOR))?;

    let msg = sub_matches
        .value_of(ARG_COMMIT_MESSAGE)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_COMMIT_MESSAGE))?;

    let cs_ids = rsync(
        &ctx,
        &repo,
        cs_id,
        from_dir,
        to_dir,
        author.to_string(),
        msg.to_string(),
        Limits::new(sub_matches),
        Options::new(sub_matches)?,
    )
    .await?;

    let result_cs_id = cs_ids
        .last()
        .copied()
        .ok_or_else(|| anyhow!("nothing to move!"))?;

    println!("{}", result_cs_id);

    Ok(())
}

#[derive(Copy, Clone, Debug, Default)]
struct Limits {
    file_num_limit: Option<NonZeroU64>,
    total_size_limit: Option<NonZeroU64>,
    lfs_threshold: Option<NonZeroU64>,
}

impl Limits {
    pub fn new(sub_m: &ArgMatches<'_>) -> Self {
        let maybe_file_num_limit = args::get_and_parse_opt::<NonZeroU64>(sub_m, ARG_FILE_NUM_LIMIT);
        let maybe_total_size_limit =
            args::get_and_parse_opt::<NonZeroU64>(sub_m, ARG_TOTAL_SIZE_LIMIT);
        let maybe_lfs_threshold = args::get_and_parse_opt::<NonZeroU64>(sub_m, ARG_LFS_THRESHOLD);


        Self {
            file_num_limit: maybe_file_num_limit,
            total_size_limit: maybe_total_size_limit,
            lfs_threshold: maybe_lfs_threshold,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Options {
    maybe_exclude_file_regex: Option<Regex>,
    overwrite: bool,
}

impl Options {
    pub fn new(sub_m: &ArgMatches<'_>) -> Result<Self, Error> {
        let maybe_exclude_file_regex = sub_m.value_of(ARG_EXCLUDE_FILE_REGEX);
        let maybe_exclude_file_regex = maybe_exclude_file_regex
            .map(Regex::new)
            .transpose()
            .map_err(Error::from)?;

        let overwrite = sub_m.is_present(ARG_OVERWRITE);

        Ok(Self {
            maybe_exclude_file_regex,
            overwrite,
        })
    }
}

async fn rsync(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    from_dir: MPath,
    to_dir: MPath,
    author: String,
    msg: String,
    limits: Limits,
    options: Options,
) -> Result<Vec<ChangesetId>, Error> {
    let (from_entries, to_entries) = try_join(
        list_directory(&ctx, &repo, cs_id, &from_dir),
        list_directory(&ctx, &repo, cs_id, &to_dir),
    )
    .await?;
    let from_entries = from_entries.ok_or_else(|| Error::msg("from directory does not exist!"))?;
    let to_entries = to_entries.unwrap_or_else(BTreeMap::new);

    // These are the file changes that have to be removed first
    let mut remove_file_changes = BTreeMap::new();
    // These are the file changes that have to be copied
    let mut file_changes = BTreeMap::new();
    let mut total_file_size = 0;
    for (from_suffix, fsnode_file) in from_entries {
        if let Some(ref regex) = options.maybe_exclude_file_regex {
            if from_suffix.matches_regex(&regex) {
                continue;
            }
        }

        let from_path = from_dir.join(&from_suffix);
        let to_path = to_dir.join(&from_suffix);

        if let Some(to_fsnode) = to_entries.get(&from_suffix) {
            if to_fsnode == &fsnode_file {
                continue;
            }

            if options.overwrite {
                remove_file_changes.insert(to_path.clone(), None);
            } else {
                continue;
            }
        }

        debug!(
            ctx.logger(),
            "from {}, to {}, size: {}",
            from_path,
            to_path,
            fsnode_file.size()
        );
        file_changes.insert(to_path, Some((from_path, fsnode_file)));
        if let Some(lfs_threshold) = limits.lfs_threshold {
            if fsnode_file.size() < lfs_threshold.get() {
                total_file_size += fsnode_file.size();
            } else {
                debug!(
                    ctx.logger(),
                    "size is not accounted because of lfs threshold"
                );
            }
        } else {
            total_file_size += fsnode_file.size();
        }

        if let Some(limit) = limits.file_num_limit {
            if file_changes.len() as u64 >= limit.get() {
                break;
            }
        }
        if let Some(limit) = limits.total_size_limit {
            if total_file_size as u64 > limit.get() {
                break;
            }
        }
    }

    create_changesets(
        ctx,
        repo,
        vec![remove_file_changes, file_changes],
        cs_id,
        author,
        msg,
    )
    .await
}

async fn create_changesets(
    ctx: &CoreContext,
    repo: &BlobRepo,
    file_changes: Vec<BTreeMap<MPath, Option<(MPath, FsnodeFile)>>>,
    mut parent: ChangesetId,
    author: String,
    msg: String,
) -> Result<Vec<ChangesetId>, Error> {
    let mut changesets = vec![];
    let mut cs_ids = vec![];
    for path_to_maybe_fsnodes in file_changes {
        if path_to_maybe_fsnodes.is_empty() {
            continue;
        }

        let mut fc = BTreeMap::new();
        for (to_path, maybe_fsnode) in path_to_maybe_fsnodes {
            let maybe_file_change = match maybe_fsnode {
                Some((from_path, fsnode_file)) => Some(FileChange::new(
                    *fsnode_file.content_id(),
                    *fsnode_file.file_type(),
                    fsnode_file.size(),
                    Some((from_path, parent)),
                )),
                None => None,
            };

            fc.insert(to_path, maybe_file_change);
        }

        info!(ctx.logger(), "creating csid with {} file changes", fc.len());
        let bcs = create_bonsai_changeset(vec![parent], fc, author.clone(), msg.clone())?;

        let cs_id = bcs.get_changeset_id();
        changesets.push(bcs);
        cs_ids.push(cs_id);
        parent = cs_id;
    }

    save_bonsai_changesets(changesets, ctx.clone(), repo.clone())
        .compat()
        .await?;

    Ok(cs_ids)
}

// Recursively lists all the files under `path` if this is a directory.
// If `path` does not exist then None is returned.
// Note that returned paths are RELATIVE to `path`.
async fn list_directory(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    path: &MPath,
) -> Result<Option<BTreeMap<MPath, FsnodeFile>>, Error> {
    let root = RootFsnodeId::derive(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;

    let entries = root
        .fsnode_id()
        .find_entries(ctx.clone(), repo.get_blobstore(), vec![path.clone()])
        .try_collect::<Vec<_>>()
        .await?;

    let entry = entries.get(0);

    let fsnode_id = match entry {
        Some((_, Entry::Tree(fsnode_id))) => fsnode_id,
        None => {
            return Ok(None);
        }
        Some((_, Entry::Leaf(_))) => {
            return Err(anyhow!(
                "{} is a file, but expected to be a directory",
                path
            ));
        }
    };

    let leaf_entries = fsnode_id
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .try_collect::<BTreeMap<_, _>>()
        .await?;

    Ok(Some(leaf_entries))
}

fn create_bonsai_changeset(
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    author: String,
    message: String,
) -> Result<BonsaiChangeset, Error> {
    BonsaiChangesetMut {
        parents,
        author,
        author_date: DateTime::now(),
        committer: None,
        committer_date: None,
        message,
        extra: BTreeMap::new(),
        file_changes,
    }
    .freeze()
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo_factory::new_memblob_empty;
    use maplit::hashmap;
    use tests_utils::{list_working_copy_utf8, CreateCommitContext};

    #[fbinit::compat_test]
    async fn test_list_directory(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .commit()
            .await?;

        let maybe_dir = list_directory(&ctx, &repo, cs_id, &MPath::new("dir")?).await?;
        let dir = maybe_dir.unwrap();

        assert_eq!(
            dir.keys().collect::<Vec<_>>(),
            vec![&MPath::new("a")?, &MPath::new("b")?]
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "a")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let new_cs_id = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options::default(),
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, new_cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "a".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
                MPath::new("dir_to/a")? => "dontoverwrite".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
                MPath::new("dir_to/c")? => "c".to_string(),
            }
        );
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_with_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "a")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let limit = Limits {
            file_num_limit: NonZeroU64::new(1),
            total_size_limit: None,
            lfs_threshold: None,
        };
        let first_cs_id = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            limit.clone(),
            Options::default(),
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, first_cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "a".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
                MPath::new("dir_to/a")? => "dontoverwrite".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
            }
        );

        let second_cs_id = rsync(
            &ctx,
            &repo,
            first_cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            limit,
            Options::default(),
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, second_cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "a".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
                MPath::new("dir_to/a")? => "dontoverwrite".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
                MPath::new("dir_to/c")? => "c".to_string(),
            }
        );
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_with_excludes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/BUCK", "buck")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/TARGETS", "targets")
            .add_file("dir_from/subdir/TARGETS", "targets")
            .add_file("dir_from/c.bzl", "bzl")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let cs_id = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                maybe_exclude_file_regex: Some(Regex::new("(BUCK|.*\\.bzl|TARGETS)$")?),
                ..Default::default()
            },
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/BUCK")? => "buck".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/TARGETS")? => "targets".to_string(),
                MPath::new("dir_from/subdir/TARGETS")? => "targets".to_string(),
                MPath::new("dir_from/c.bzl")? => "bzl".to_string(),
                MPath::new("dir_to/a")? => "dontoverwrite".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_with_file_size_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aaaaaaaaaa")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .commit()
            .await?;

        let first_cs_id = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits {
                file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: None,
            },
            Options::default(),
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, first_cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "aaaaaaaaaa".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
                MPath::new("dir_to/a")? => "aaaaaaaaaa".to_string(),
            }
        );

        let second_cs_id = rsync(
            &ctx,
            &repo,
            first_cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits {
                file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: None,
            },
            Options::default(),
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, second_cs_id,).await?,
            hashmap! {
                MPath::new("dir_to/a")? => "aaaaaaaaaa".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
                MPath::new("dir_to/c")? => "c".to_string(),
                MPath::new("dir_from/a")? => "aaaaaaaaaa".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_with_file_size_limit_and_lfs_threshold(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aaaaaaaaaa")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .commit()
            .await?;

        let cs_ids = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits {
                file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: NonZeroU64::new(2),
            },
            Options::default(),
        )
        .await?;
        assert_eq!(cs_ids.len(), 1);
        let cs_id = cs_ids.last().copied().unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, cs_id,).await?,
            hashmap! {
                MPath::new("dir_to/a")? => "aaaaaaaaaa".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
                MPath::new("dir_to/c")? => "c".to_string(),
                MPath::new("dir_from/a")? => "aaaaaaaaaa".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_from/c")? => "c".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_rsync_with_overwrite(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aa")
            .add_file("dir_from/b", "b")
            .add_file("dir_to/a", "a")
            .add_file("dir_to/b", "b")
            .commit()
            .await?;

        // No overwrite - nothing should be copied
        let cs_ids = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                overwrite: false,
                ..Default::default()
            },
        )
        .await?;
        assert!(cs_ids.is_empty());

        // Use overwrite - it should create two commits.
        // First commit removes dir_to/a, second commit copies dir_form/a to dir_to/a
        let cs_ids = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                overwrite: true,
                ..Default::default()
            },
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, *cs_ids.get(0).unwrap()).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "aa".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
            }
        );

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, *cs_ids.last().unwrap()).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "aa".to_string(),
                MPath::new("dir_from/b")? => "b".to_string(),
                MPath::new("dir_to/a")? => "aa".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
            }
        );

        Ok(())
    }
}

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
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::try_join,
    TryStreamExt,
};

use blobrepo::BlobRepo;
use cmdlib::{args, helpers};
use context::CoreContext;
use manifest::{Entry, ManifestOps};
use mononoke_types::{
    fsnode::FsnodeFile, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange,
    MPath,
};
use slog::{info, Logger};
use std::{collections::BTreeMap, num::NonZeroU64};

use crate::error::SubcommandError;

pub const ARG_COMMIT_AUTHOR: &str = "commit-author";
pub const ARG_COMMIT_MESSAGE: &str = "commit-message";
pub const ARG_CSID: &str = "csid";
pub const ARG_LIMIT: &str = "limit";
pub const ARG_FROM_DIR: &str = "from-dir";
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
            Arg::with_name(ARG_LIMIT)
                .long(ARG_LIMIT)
                .help("limit the number of files moved in a commit")
                .takes_value(true)
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

    let maybe_limit = args::get_and_parse_opt::<NonZeroU64>(sub_matches, ARG_LIMIT);
    let result_cs_id = rsync(
        &ctx,
        &repo,
        cs_id,
        from_dir,
        to_dir,
        author.to_string(),
        msg.to_string(),
        maybe_limit,
    )
    .await?;

    println!("{}", result_cs_id);

    Ok(())
}

async fn rsync(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    from_dir: MPath,
    to_dir: MPath,
    author: String,
    msg: String,
    maybe_limit: Option<NonZeroU64>,
) -> Result<ChangesetId, Error> {
    let (from_entries, to_entries) = try_join(
        list_directory(&ctx, &repo, cs_id, &from_dir),
        list_directory(&ctx, &repo, cs_id, &to_dir),
    )
    .await?;
    let from_entries = from_entries.ok_or_else(|| Error::msg("from directory does not exist!"))?;
    let to_entries = to_entries.unwrap_or_else(BTreeMap::new);

    let mut file_changes = BTreeMap::new();
    for (from_suffix, fsnode_file) in from_entries {
        if !to_entries.contains_key(&from_suffix) {
            let from_path = from_dir.join(&from_suffix);
            let to_path = to_dir.join(&from_suffix);

            let file_change = FileChange::new(
                *fsnode_file.content_id(),
                *fsnode_file.file_type(),
                fsnode_file.size(),
                Some((from_path, cs_id)),
            );
            file_changes.insert(to_path, Some(file_change));
            if let Some(limit) = maybe_limit {
                if file_changes.len() as u64 >= limit.get() {
                    break;
                }
            }
        }
    }

    info!(
        ctx.logger(),
        "creating csid with {} file changes",
        file_changes.len()
    );
    let parents = vec![cs_id];
    let bcs = create_bonsai_changeset(parents, file_changes, author, msg)?;
    let result_cs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await?;

    Ok(result_cs_id)
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
        .compat()
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
        .compat()
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
            None,
        )
        .await?;

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

        let limit = NonZeroU64::new(1);
        let first_cs_id = rsync(
            &ctx,
            &repo,
            cs_id,
            MPath::new("dir_from")?,
            MPath::new("dir_to")?,
            "author".to_string(),
            "msg".to_string(),
            limit,
        )
        .await?;

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
        )
        .await?;

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
}

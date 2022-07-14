/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::save_bonsai_changesets;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::future::try_join;
use futures::TryStreamExt;

use blobrepo::BlobRepo;
use commit_transformation::copy_file_contents;
use context::CoreContext;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use regex::Regex;
use slog::debug;
use slog::info;
use sorted_vector_map::SortedVectorMap;
use std::collections::BTreeMap;
use std::num::NonZeroU64;

#[derive(Copy, Clone, Debug, Default)]
pub struct Limits {
    pub total_file_num_limit: Option<NonZeroU64>,
    pub total_size_limit: Option<NonZeroU64>,
    pub lfs_threshold: Option<NonZeroU64>,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub maybe_exclude_file_regex: Option<Regex>,
    pub overwrite: bool,
}

pub async fn copy(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    target_repo: &BlobRepo,
    source_cs_id: ChangesetId,
    target_cs_id: ChangesetId,
    from_to_dirs: Vec<(MPath, MPath)>,
    author: String,
    msg: String,
    limits: Limits,
    options: Options,
    skip_overwrite_warning: Option<&dyn Fn(&CoreContext, &MPath)>,
) -> Result<Vec<ChangesetId>, Error> {
    // These are the file changes that have to be removed first
    let mut remove_file_changes = BTreeMap::new();
    // These are the file changes that have to be copied
    let mut file_changes = BTreeMap::new();
    let mut total_file_size = 0;
    let mut contents_to_upload = vec![];
    let same_repo = source_repo.get_repoid() == target_repo.get_repoid();

    for (from_dir, to_dir) in from_to_dirs {
        let (from_entries, to_entries) = try_join(
            list_directory(ctx, source_repo, source_cs_id, &from_dir),
            list_directory(ctx, target_repo, target_cs_id, &to_dir),
        )
        .await?;
        let from_entries =
            from_entries.ok_or_else(|| Error::msg("from directory does not exist!"))?;
        let to_entries = to_entries.unwrap_or_else(BTreeMap::new);

        for (from_suffix, fsnode_file) in from_entries {
            if let Some(ref regex) = options.maybe_exclude_file_regex {
                if from_suffix.matches_regex(regex) {
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
                    if let Some(skip_overwrite_warning) = skip_overwrite_warning {
                        skip_overwrite_warning(ctx, &to_path);
                    }
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

            if !same_repo {
                contents_to_upload.push(fsnode_file.content_id().clone());
            }

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

            if let Some(limit) = limits.total_file_num_limit {
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
    }

    if !same_repo {
        debug!(
            ctx.logger(),
            "Copying {} files contents from {} to {}",
            contents_to_upload.len(),
            source_repo.name(),
            target_repo.name()
        );
        copy_file_contents(ctx, source_repo, target_repo, contents_to_upload, |_| {}).await?;
    }

    create_changesets(
        ctx,
        target_repo,
        vec![remove_file_changes, file_changes],
        target_cs_id,
        author,
        msg,
        same_repo, /* record_copy_from */
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
    record_copy_from: bool,
) -> Result<Vec<ChangesetId>, Error> {
    let mut changesets = vec![];
    let mut cs_ids = vec![];
    for path_to_maybe_fsnodes in file_changes {
        if path_to_maybe_fsnodes.is_empty() {
            continue;
        }

        let mut fc = BTreeMap::new();
        for (to_path, maybe_fsnode) in path_to_maybe_fsnodes {
            let file_change = match maybe_fsnode {
                Some((from_path, fsnode_file)) => {
                    let copy_from = if record_copy_from {
                        Some((from_path, parent))
                    } else {
                        None
                    };
                    FileChange::tracked(
                        *fsnode_file.content_id(),
                        *fsnode_file.file_type(),
                        fsnode_file.size(),
                        copy_from,
                    )
                }
                None => FileChange::Deletion,
            };

            fc.insert(to_path, file_change);
        }

        info!(ctx.logger(), "creating csid with {} file changes", fc.len());
        let bcs = create_bonsai_changeset(vec![parent], fc.into(), author.clone(), msg.clone())?;

        let cs_id = bcs.get_changeset_id();
        changesets.push(bcs);
        cs_ids.push(cs_id);
        parent = cs_id;
    }

    save_bonsai_changesets(changesets, ctx.clone(), repo).await?;

    Ok(cs_ids)
}

pub async fn remove_excessive_files(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    target_repo: &BlobRepo,
    source_cs_id: ChangesetId,
    target_cs_id: ChangesetId,
    from_to_dirs: Vec<(MPath, MPath)>,
    author: String,
    msg: String,
    maybe_total_file_num_limit: Option<NonZeroU64>,
) -> Result<ChangesetId, Error> {
    let mut to_delete = BTreeMap::new();

    for (from_dir, to_dir) in from_to_dirs {
        let (from_entries, to_entries) = try_join(
            list_directory(ctx, source_repo, source_cs_id, &from_dir),
            list_directory(ctx, target_repo, target_cs_id, &to_dir),
        )
        .await?;
        let from_entries =
            from_entries.ok_or_else(|| Error::msg("from directory does not exist!"))?;
        let to_entries = to_entries.unwrap_or_else(BTreeMap::new);

        for to_suffix in to_entries.keys() {
            if !from_entries.contains_key(to_suffix) {
                let to_path = to_dir.join(to_suffix);
                to_delete.insert(to_path, None);
                if let Some(limit) = maybe_total_file_num_limit {
                    if to_delete.len() as u64 >= limit.get() {
                        break;
                    }
                }
            }
        }
    }

    let cs_ids = create_changesets(
        ctx,
        target_repo,
        vec![to_delete],
        target_cs_id,
        author,
        msg,
        false, /* record_copy_from */
    )
    .await?;

    cs_ids
        .last()
        .copied()
        .ok_or_else(|| anyhow!("nothing to remove!"))
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
    let root = RootFsnodeId::derive(ctx, repo, cs_id).await?;

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
    file_changes: SortedVectorMap<MPath, FileChange>,
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
        extra: Default::default(),
        file_changes,
        is_snapshot: false,
    }
    .freeze()
}

#[cfg(test)]
mod test {
    use super::*;
    use blobstore::StoreLoadable;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::RepositoryId;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_list_directory(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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

    #[fbinit::test]
    async fn test_rsync_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "a")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let new_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options::default(),
            None,
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

    #[fbinit::test]
    async fn test_rsync_multiple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from_1/a", "a")
            .add_file("dir_from_1/b", "b")
            .add_file("dir_from_1/c", "c")
            .add_file("dir_to_1/a", "dontoverwrite")
            .add_file("dir_from_2/aa", "aa")
            .add_file("dir_from_2/bb", "bb")
            .add_file("dir_from_2/cc", "cc")
            .commit()
            .await?;

        let new_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![
                (MPath::new("dir_from_1")?, MPath::new("dir_to_1")?),
                (MPath::new("dir_from_2")?, MPath::new("dir_to_2")?),
            ],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options::default(),
            None,
        )
        .await?
        .last()
        .copied()
        .unwrap();

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, new_cs_id,).await?,
            hashmap! {
                MPath::new("dir_from_1/a")? => "a".to_string(),
                MPath::new("dir_from_1/b")? => "b".to_string(),
                MPath::new("dir_from_1/c")? => "c".to_string(),
                MPath::new("dir_to_1/a")? => "dontoverwrite".to_string(),
                MPath::new("dir_to_1/b")? => "b".to_string(),
                MPath::new("dir_to_1/c")? => "c".to_string(),

                MPath::new("dir_from_2/aa")? => "aa".to_string(),
                MPath::new("dir_from_2/bb")? => "bb".to_string(),
                MPath::new("dir_from_2/cc")? => "cc".to_string(),
                MPath::new("dir_to_2/aa")? => "aa".to_string(),
                MPath::new("dir_to_2/bb")? => "bb".to_string(),
                MPath::new("dir_to_2/cc")? => "cc".to_string(),
            }
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_rsync_with_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "a")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let limit = Limits {
            total_file_num_limit: NonZeroU64::new(1),
            total_size_limit: None,
            lfs_threshold: None,
        };
        let first_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            limit.clone(),
            Options::default(),
            None,
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

        let second_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            first_cs_id,
            first_cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            limit,
            Options::default(),
            None,
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

    #[fbinit::test]
    async fn test_rsync_with_excludes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/BUCK", "buck")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/TARGETS", "targets")
            .add_file("dir_from/subdir/TARGETS", "targets")
            .add_file("dir_from/c.bzl", "bzl")
            .add_file("dir_to/a", "dontoverwrite")
            .commit()
            .await?;

        let cs_id = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                maybe_exclude_file_regex: Some(Regex::new("(BUCK|.*\\.bzl|TARGETS)$")?),
                ..Default::default()
            },
            None,
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

    #[fbinit::test]
    async fn test_rsync_with_file_size_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aaaaaaaaaa")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .commit()
            .await?;

        let first_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits {
                total_file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: None,
            },
            Options::default(),
            None,
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

        let second_cs_id = copy(
            &ctx,
            &repo,
            &repo,
            first_cs_id,
            first_cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits {
                total_file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: None,
            },
            Options::default(),
            None,
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

    #[fbinit::test]
    async fn test_rsync_with_file_size_limit_and_lfs_threshold(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aaaaaaaaaa")
            .add_file("dir_from/b", "b")
            .add_file("dir_from/c", "c")
            .commit()
            .await?;

        let cs_ids = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits {
                total_file_num_limit: None,
                total_size_limit: NonZeroU64::new(5),
                lfs_threshold: NonZeroU64::new(2),
            },
            Options::default(),
            None,
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

    #[fbinit::test]
    async fn test_rsync_with_overwrite(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "aa")
            .add_file("dir_from/b", "b")
            .add_file("dir_to/a", "a")
            .add_file("dir_to/b", "b")
            .commit()
            .await?;

        // No overwrite - nothing should be copied
        let cs_ids = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                overwrite: false,
                ..Default::default()
            },
            None,
        )
        .await?;
        assert!(cs_ids.is_empty());

        // Use overwrite - it should create two commits.
        // First commit removes dir_to/a, second commit copies dir_form/a to dir_to/a
        let cs_ids = copy(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                overwrite: true,
                ..Default::default()
            },
            None,
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

        let copy_bcs = cs_ids
            .last()
            .expect("changeset is expected to exist")
            .load(&ctx, &repo.get_blobstore())
            .await?;
        let file_changes = copy_bcs.file_changes_map();
        let a_change = match file_changes
            .get(&MPath::new("dir_to/a")?)
            .expect("change to dir_to/a expected to be present in the map")
        {
            FileChange::Change(tc) => tc,
            _ => panic!("change to dir_to/a expected to not be None"),
        };
        // Ensure that there is copy-from inserted when copying within the repo
        assert!(a_change.copy_from().is_some());

        Ok(())
    }

    #[fbinit::test]
    async fn test_delete_excessive_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from/a", "a")
            .add_file("dir_to/a", "a")
            .add_file("dir_to/b", "b")
            .add_file("dir_to/c/d", "c/d")
            .commit()
            .await?;

        let cs_id = remove_excessive_files(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            None,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, cs_id,).await?,
            hashmap! {
                MPath::new("dir_from/a")? => "a".to_string(),
                MPath::new("dir_to/a")? => "a".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_delete_excessive_files_multiple_dirs(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_from_1/a", "a")
            .add_file("dir_to_1/a", "a")
            .add_file("dir_to_1/b", "b")
            .add_file("dir_to_1/c/d", "c/d")
            .add_file("dir_from_2/a", "a")
            .add_file("dir_to_2/a", "a")
            .add_file("dir_to_2/b", "b")
            .commit()
            .await?;

        let cs_id = remove_excessive_files(
            &ctx,
            &repo,
            &repo,
            cs_id,
            cs_id,
            vec![
                (MPath::new("dir_from_1")?, MPath::new("dir_to_1")?),
                (MPath::new("dir_from_2")?, MPath::new("dir_to_2")?),
            ],
            "author".to_string(),
            "msg".to_string(),
            None,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(&ctx, &repo, cs_id,).await?,
            hashmap! {
                MPath::new("dir_from_1/a")? => "a".to_string(),
                MPath::new("dir_to_1/a")? => "a".to_string(),
                MPath::new("dir_from_2/a")? => "a".to_string(),
                MPath::new("dir_to_2/a")? => "a".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_delete_excessive_files_xrepo(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new(fb)?;
        let source_repo = factory.with_id(RepositoryId::new(0)).build()?;
        let target_repo = factory.with_id(RepositoryId::new(1)).build()?;

        let source_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("dir_from/a", "a")
            .commit()
            .await?;

        let target_cs_id = CreateCommitContext::new_root(&ctx, &target_repo)
            .add_file("dir_to/a", "a")
            .add_file("dir_to/b", "b")
            .add_file("dir_to/c/d", "c/d")
            .commit()
            .await?;

        let cs_id = remove_excessive_files(
            &ctx,
            &source_repo,
            &target_repo,
            source_cs_id,
            target_cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            None,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(&ctx, &target_repo, cs_id).await?,
            hashmap! {
                MPath::new("dir_to/a")? => "a".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_xrepo_rsync_with_overwrite(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new(fb)?;
        let source_repo = factory.with_id(RepositoryId::new(0)).build()?;
        let target_repo = factory.with_id(RepositoryId::new(1)).build()?;

        let source_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("dir_from/a", "aa")
            .add_file("dir_from/b", "b")
            .add_file("source_random_file", "sr")
            .commit()
            .await?;

        let target_cs_id = CreateCommitContext::new_root(&ctx, &target_repo)
            .add_file("dir_to/a", "different")
            .add_file("target_random_file", "tr")
            .commit()
            .await?;

        let cs_ids = copy(
            &ctx,
            &source_repo,
            &target_repo,
            source_cs_id,
            target_cs_id,
            vec![(MPath::new("dir_from")?, MPath::new("dir_to")?)],
            "author".to_string(),
            "msg".to_string(),
            Limits::default(),
            Options {
                overwrite: true,
                ..Default::default()
            },
            None,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(&ctx, &target_repo, *cs_ids.get(0).unwrap()).await?,
            hashmap! {
                MPath::new("target_random_file")? => "tr".to_string(),
            }
        );

        assert_eq!(
            list_working_copy_utf8(&ctx, &target_repo, *cs_ids.get(1).unwrap()).await?,
            hashmap! {
                MPath::new("dir_to/a")? => "aa".to_string(),
                MPath::new("dir_to/b")? => "b".to_string(),
                MPath::new("target_random_file")? => "tr".to_string(),
            }
        );

        assert_eq!(
            target_repo
                .get_changeset_parents_by_bonsai(ctx.clone(), *cs_ids.get(0).unwrap())
                .await?,
            vec![target_cs_id],
        );

        let copy_bcs = cs_ids
            .get(1)
            .expect("changeset is expected to exist")
            .load(&ctx, &target_repo.get_blobstore())
            .await?;
        let file_changes = copy_bcs.file_changes_map();
        let a_change = match file_changes
            .get(&MPath::new("dir_to/a")?)
            .expect("change to dir_to/a expected to be present in the map")
        {
            FileChange::Change(tc) => tc,
            _ => panic!("change to dir_to/a expected to not be None"),
        };
        // Ensure that there's no copy-from inserted when copying from another repo
        assert!(a_change.copy_from().is_none());

        Ok(())
    }
}

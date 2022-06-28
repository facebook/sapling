/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! DrawDAG for Integration Tests
//!
//! A DrawDAG specification consists of an ASCII graph (either left-to-right
//! or bottom-to-top), and a series of comments that define additional
//! properties for each commit.
//!
//! Valid properties are:
//!
//! * Set a known changeset id for an already-existing commit
//!     # exists: COMMIT id
//!
//! * Set a bookmark on a commit
//!     # bookmark: COMMIT name
//!
//! * Set the content of a file.
//!     # modify: COMMIT path/to/file "content"
//!
//! * Mark a file as deleted.
//!     # delete: COMMIT path/to/file
//!
//! * Forget file that was about to be added (useful for getting rid of files
//!   that are added by default):
//!     # forget: COMMIT path/to/file
//!
//! Paths can be surrounded by quotes if they contain special characters.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Display;
use std::io::Write;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blame::RootBlameV2;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use changeset_info::ChangesetInfo;
use clap::Parser;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BatchDeriveOptions;
use derived_data_manager::BonsaiDerivable;
use fastlog::RootFastlog;
use fsnodes::RootFsnodeId;
use futures::try_join;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use tests_utils::drawdag::extend_from_dag_with_changes;
use tests_utils::drawdag::ChangeFn;
use tests_utils::CommitIdentifier;
use tests_utils::CreateCommitContext;
use tokio::io::AsyncReadExt;
use topo_sort::sort_topological;
use unodes::RootUnodeManifestId;

/// Create commits from a drawn DAG.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Disable creation of default files in each commit
    #[clap(long)]
    no_default_files: bool,

    /// Derive all derived data types for all commits
    #[clap(long)]
    derive_all: bool,

    /// Print hashes in HG format instead of bonsai
    #[clap(long)]
    print_hg_hashes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Action {
    Exists {
        name: String,
        id: ChangesetId,
    },
    Bookmark {
        name: String,
        bookmark: BookmarkName,
    },
    Change {
        name: String,
        change: ChangeAction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChangeAction {
    Modify {
        path: Vec<u8>,
        content: Vec<u8>,
    },
    Delete {
        path: Vec<u8>,
    },
    Forget {
        path: Vec<u8>,
    },
    Extra {
        key: String,
        value: Vec<u8>,
    },
    Copy {
        path: Vec<u8>,
        content: Vec<u8>,
        parent: String,
        parent_path: Vec<u8>,
    },
}

impl Action {
    fn new(spec: &str) -> Result<Self> {
        if let Some((key, args)) = spec.trim().split_once(':') {
            let args = ActionArg::parse_args(args)
                .with_context(|| format!("Failed to parse args for '{}'", key))?;
            match (key, args.as_slice()) {
                ("exists", [name, id]) => {
                    let name = name.to_string()?;
                    let id = id.to_string()?.parse()?;
                    Ok(Action::Exists { name, id })
                }
                ("bookmark", [name, bookmark]) => {
                    let name = name.to_string()?;
                    let bookmark = bookmark.to_string()?.parse()?;
                    Ok(Action::Bookmark { name, bookmark })
                }
                ("modify", [name, path, content]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let content = content.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Modify { path, content },
                    })
                }
                ("delete", [name, path]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Delete { path },
                    })
                }
                ("forget", [name, path]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Forget { path },
                    })
                }
                ("extra", [name, key, value]) => {
                    let name = name.to_string()?;
                    let key = key.to_string()?;
                    let value = value.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Extra { key, value },
                    })
                }
                ("copy", [name, path, content, parent, parent_path]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let content = content.to_bytes();
                    let parent = parent.to_string()?;
                    let parent_path = parent_path.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Copy {
                            path,
                            content,
                            parent,
                            parent_path,
                        },
                    })
                }
                _ => Err(anyhow!("Invalid spec for key: {}", key)),
            }
        } else {
            Err(anyhow!("Invalid spec: {}", spec))
        }
    }
}

struct ActionArg(Vec<u8>);

impl ActionArg {
    fn new() -> Self {
        ActionArg(Vec::new())
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn to_string(&self) -> Result<String> {
        let s = std::str::from_utf8(&self.0)
            .context("Expected UTF-8 string for drawdag action argument")?;
        Ok(s.to_string())
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn push(&mut self, ch: char) {
        let mut buf = [0; 4];
        self.0
            .extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }

    fn push_byte(&mut self, byte: u8) {
        self.0.push(byte)
    }

    fn push_hex(&mut self, mut iter: impl Iterator<Item = char>) -> Result<()> {
        if let (Some(top_hex), Some(bottom_hex)) = (iter.next(), iter.next()) {
            if let (Some(top_digit), Some(bottom_digit)) =
                (top_hex.to_digit(16), bottom_hex.to_digit(16))
            {
                self.push_byte((top_digit * 0x10 + bottom_digit) as u8);
                return Ok(());
            }
        }
        Err(anyhow!("Expected two hex digits"))
    }

    fn parse_args(args: &str) -> Result<Vec<Self>> {
        let mut iter = args.trim().chars();
        let mut args = Vec::new();
        let mut arg = ActionArg::new();
        let mut in_quotes = false;
        while let Some(ch) = iter.next() {
            if in_quotes {
                match ch {
                    '"' => in_quotes = false,
                    '\\' => match iter
                        .next()
                        .ok_or_else(|| anyhow!("Unexpected end-of-line after '\\'"))?
                    {
                        '\\' => arg.push('\\'),
                        'r' => arg.push('\r'),
                        'n' => arg.push('\n'),
                        't' => arg.push('\t'),
                        'f' => arg.push('\u{0C}'),
                        'b' => arg.push('\u{08}'),
                        '"' => arg.push('"'),
                        'x' => arg.push_hex(&mut iter)?,
                        esc => return Err(anyhow!("Unexpected escape sequence: '\\{}'", esc)),
                    },
                    ch => arg.push(ch),
                }
            } else {
                match ch {
                    '"' => in_quotes = true,
                    ch if ch.is_whitespace() => {
                        if !arg.is_empty() {
                            args.push(arg);
                            arg = ActionArg::new();
                        }
                    }
                    ch if ch.is_alphanumeric() || "_./".contains(ch) => {
                        arg.push(ch);
                    }
                    ch => return Err(anyhow!("Unexpected character: '{}'", ch)),
                }
            }
        }
        if in_quotes {
            return Err(anyhow!("Unterminated string literal"));
        }
        if !arg.is_empty() {
            args.push(arg);
        }
        Ok(args)
    }
}

fn print_name_hash_pairs(pairs: impl IntoIterator<Item = (String, impl Display)>) -> Result<()> {
    for (name, id) in pairs.into_iter() {
        writeln!(std::io::stdout(), "{}={}", name, id)?;
    }
    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: BlobRepo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    // Read DAG from stdin
    let mut input = String::new();
    tokio::io::stdin().read_to_string(&mut input).await?;

    let mut dag_buffer = String::new();
    let mut actions = Vec::new();
    for line in input.lines() {
        if let Some((dag_line, comment)) = line.split_once('#') {
            dag_buffer.push_str(dag_line);
            dag_buffer.push('\n');
            actions.push(Action::new(comment)?);
        } else {
            dag_buffer.push_str(line);
            dag_buffer.push('\n');
        }
    }

    let mut existing: BTreeMap<String, ChangesetId> = BTreeMap::new();
    let mut commit_changes: BTreeMap<String, Vec<ChangeAction>> = BTreeMap::new();
    let mut bookmarks: BTreeMap<BookmarkName, String> = BTreeMap::new();

    for action in actions {
        match action {
            Action::Exists { name, id } => {
                existing.insert(name, id);
            }
            Action::Bookmark { name, bookmark } => {
                bookmarks.insert(bookmark, name);
            }
            Action::Change { name, change } => {
                commit_changes
                    .entry(name)
                    .or_insert_with(Vec::new)
                    .push(change);
            }
        }
    }

    let mut change_fns = BTreeMap::new();
    for (name, changes) in commit_changes {
        let apply: Box<ChangeFn<BlobRepo>> = Box::new(
            move |c: CreateCommitContext<BlobRepo>,
                  committed: &'_ BTreeMap<String, ChangesetId>| {
                apply_changes(c, committed, changes)
            },
        );
        change_fns.insert(name, apply);
    }

    let (commits, dag) = extend_from_dag_with_changes(
        &ctx,
        &repo,
        &dag_buffer,
        change_fns,
        existing,
        !args.no_default_files,
    )
    .await?;

    if !bookmarks.is_empty() {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        for (bookmark, name) in bookmarks {
            let target = commits
                .get(&name)
                .ok_or_else(|| anyhow!("No commit {} for bookmark {}", name, bookmark))?;
            let old_value = repo
                .bookmarks()
                .get(ctx.clone(), &bookmark)
                .await
                .with_context(|| format!("Failed to resolve bookmark '{}'", bookmark))?;
            // It's better to update/create rather than force_set which doesn't
            // save the old cid to the bookmark update log. (So it looks like
            // creation but it's update)
            match old_value {
                Some(old_value) => txn.update(
                    &bookmark,
                    *target,
                    old_value,
                    BookmarkUpdateReason::TestMove,
                ),
                None => txn.create(&bookmark, *target, BookmarkUpdateReason::TestMove),
            }?;
        }
        txn.commit().await?;
    }

    let any_derivation_needed = args.derive_all | args.print_hg_hashes;
    if any_derivation_needed {
        let dag = dag
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        let sorted = sort_topological(&dag).ok_or_else(|| anyhow!("Graph has a cycle"))?;
        let csids = sorted
            .into_iter()
            .map(|name| {
                commits
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| anyhow!("No commit found for {}", name))
            })
            .collect::<Result<Vec<_>>>()?;

        if args.derive_all {
            derive_all(&ctx, &repo, &csids).await?;
        } else {
            derive::<MappedHgChangesetId>(&ctx, &repo, &csids).await?;
        }
    }

    if args.print_hg_hashes {
        let mapping: HashMap<_, _> = repo
            .bonsai_hg_mapping()
            .get(&ctx, commits.values().copied().collect::<Vec<_>>().into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.hg_cs_id))
            .collect();
        let commits = commits
            .into_iter()
            .map(|(name, id)| {
                mapping
                    .get(&id)
                    .ok_or_else(|| anyhow!("Couldn't translate {}={} to hg", name, id))
                    .map(|hg_id| (name, hg_id))
            })
            .collect::<Result<Vec<_>>>()?;
        print_name_hash_pairs(commits)?;
    } else {
        print_name_hash_pairs(commits)?;
    }

    Ok(())
}

fn apply_changes<'a>(
    mut c: CreateCommitContext<'a, BlobRepo>,
    committed: &'_ BTreeMap<String, ChangesetId>,
    changes: Vec<ChangeAction>,
) -> CreateCommitContext<'a, BlobRepo> {
    for change in changes {
        match change {
            ChangeAction::Modify { path, content, .. } => c = c.add_file(path.as_slice(), content),
            ChangeAction::Delete { path, .. } => c = c.delete_file(path.as_slice()),
            ChangeAction::Forget { path, .. } => c = c.forget_file(path.as_slice()),
            ChangeAction::Extra { key, value, .. } => c = c.add_extra(key, value),
            ChangeAction::Copy {
                path,
                content,
                parent,
                parent_path,
                ..
            } => {
                let parent: CommitIdentifier =
                    committed.get(&parent).map_or(parent.into(), |&c| c.into());
                c = c.add_file_with_copy_info(
                    path.as_slice(),
                    content,
                    (parent, parent_path.as_slice()),
                )
            }
        }
    }
    c
}

async fn derive<D: BonsaiDerivable>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: &[ChangesetId],
) -> Result<()> {
    let mgr = repo.repo_derived_data().manager();
    mgr.backfill_batch::<D>(
        ctx,
        csids.to_vec(),
        BatchDeriveOptions::Parallel { gap_size: None },
        None,
    )
    .await
    .with_context(|| format!("Failed to derive {}", D::NAME))?;
    Ok(())
}

async fn derive_all(ctx: &CoreContext, repo: &BlobRepo, csids: &[ChangesetId]) -> Result<()> {
    let mercurial = async {
        derive::<MappedHgChangesetId>(ctx, repo, csids).await?;
        derive::<FilenodesOnlyPublic>(ctx, repo, csids).await?;
        Ok::<_, Error>(())
    };
    let unodes = async {
        derive::<RootUnodeManifestId>(ctx, repo, csids).await?;
        try_join!(
            derive::<RootBlameV2>(ctx, repo, csids),
            derive::<RootDeletedManifestV2Id>(ctx, repo, csids),
            derive::<RootFastlog>(ctx, repo, csids),
        )?;
        Ok::<_, Error>(())
    };
    try_join!(
        mercurial,
        unodes,
        derive::<RootFsnodeId>(ctx, repo, csids),
        derive::<RootSkeletonManifestId>(ctx, repo, csids),
        derive::<ChangesetInfo>(ctx, repo, csids),
    )?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_action_specs() -> Result<()> {
        assert_eq!(
            Action::new(
                "exists: A aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )?,
            Action::Exists {
                name: "A".to_string(),
                id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse()?,
            }
        );
        assert_eq!(
            Action::new("bookmark: \"A-bookmark\" \"main\"/\"bookmark\"")?,
            Action::Bookmark {
                name: "A-bookmark".to_string(),
                bookmark: "main/bookmark".parse()?,
            }
        );
        assert_eq!(
            Action::new(
                "modify: _1 path/to/file \"this has \\xaa content\\n\\ton \\x02 lines with \\\"quotes\\\"\""
            )?,
            Action::Change {
                name: "_1".to_string(),
                change: ChangeAction::Modify {
                    path: b"path/to/file".to_vec(),
                    content: b"this has \xaa content\n\ton \x02 lines with \"quotes\"".to_vec(),
                }
            }
        );
        assert_eq!(
            Action::new("delete: x path/\"to a deleted file\"")?,
            Action::Change {
                name: "x".to_string(),
                change: ChangeAction::Delete {
                    path: b"path/to a deleted file".to_vec(),
                }
            }
        );
        Ok(())
    }
}

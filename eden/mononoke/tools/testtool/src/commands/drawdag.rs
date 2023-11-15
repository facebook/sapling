/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! DrawDAG for Integration Tests
//!
//! A DrawDAG specification consists of an ASCII graph (either left-to-right
//! or bottom-to-top), and a series of comments that define actions that apply
//! to that graph.
//!
//! See documentation of `Action` for actions that affect the repository, and
//! `ChangeAction` for actions that change commits.
//!
//! Values that contain special characters can be surrounded by quotes.
//! Values that require binary data can prefix a hex string with `&`, e.g.
//! `&face` becomes a two byte string with the values `FA CE`.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Display;
use std::io::Write;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use basename_suffix_skeleton_manifest::RootBasenameSuffixSkeletonManifest;
use blame::RootBlameV2;
use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use changeset_info::ChangesetInfo;
use clap::Parser;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BatchDeriveOptions;
use derived_data_manager::BonsaiDerivable;
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::try_join;
use mercurial_derivation::MappedHgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileType;
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

/// An action that affects the graph.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Action {
    /// Set a known changeset id for an already-existing commit.  This commit
    /// will not be created, but other commits in the graph that relate to it
    /// will be related to this existing commit.
    ///
    ///     # exists: COMMIT id
    Exists { name: String, id: ChangesetId },
    /// Set a bookmark on a commit
    ///
    ///     # bookmark: COMMIT name
    Bookmark { name: String, bookmark: BookmarkKey },
    /// Change a commit
    Change { name: String, change: ChangeAction },
}

/// An action that changes one of the commits in the graph.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ChangeAction {
    /// Set the content of a file (optionally with file type).
    ///
    ///     # modify: COMMIT path/to/file [TYPE] "content"
    Modify {
        path: Vec<u8>,
        file_type: FileType,
        content: Vec<u8>,
    },
    /// Mark a file as deleted.
    ///
    ///     # delete: COMMIT path/to/file
    Delete { path: Vec<u8> },
    /// Forget file that was about to be added (useful for getting rid of files
    /// that are added by default).
    ///
    ///     # forget: COMMIT path/to/file
    Forget { path: Vec<u8> },
    /// Mark a file as a copy of another file (optionally with file type).
    ///
    ///     # copy: COMMIT path/to/file [TYPE] "content" PARENT_COMMIT_ID path/copied/from
    Copy {
        path: Vec<u8>,
        file_type: FileType,
        content: Vec<u8>,
        parent: String,
        parent_path: Vec<u8>,
    },
    /// Set a Mercurial commit extra on a commit.
    ///
    ///     # extra: COMMIT "key" "value"
    Extra { key: String, value: Vec<u8> },
    /// Set the commit message.
    ///
    ///     # message: COMMIT "message"
    Message { message: String },
    /// Set the author.
    ///
    ///     # author: COMMIT "Author Name <email@domain>"
    Author { author: String },
    /// Set the author date (in RFC3339 format).
    ///
    ///     # author_date: COMMIT "YYYY-mm-ddTHH:MM:SS+ZZ:ZZ"
    AuthorDate { author_date: DateTime },
    /// Set the committer.
    ///
    ///     # comitter: COMMIT "Committer Name <email@domain>"
    Committer { committer: String },
    /// Set the committer date (in RFC3339 format).
    ///
    ///     # committer_date: COMMIT "YYYY-mm-ddTHH:MM:SS+ZZ:ZZ"
    CommitterDate { committer_date: DateTime },
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
                ("message", [name, message]) => {
                    let name = name.to_string()?;
                    let message = message.to_string()?;
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Message { message },
                    })
                }
                ("author", [name, author]) => {
                    let name = name.to_string()?;
                    let author = author.to_string()?;
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Author { author },
                    })
                }
                ("author_date", [name, author_date]) => {
                    let name = name.to_string()?;
                    let author_date = DateTime::from_rfc3339(&author_date.to_string()?)?;
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::AuthorDate { author_date },
                    })
                }
                ("committer", [name, committer]) => {
                    let name = name.to_string()?;
                    let committer = committer.to_string()?;
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Committer { committer },
                    })
                }
                ("committer_date", [name, committer_date]) => {
                    let name = name.to_string()?;
                    let committer_date = DateTime::from_rfc3339(&committer_date.to_string()?)?;
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::CommitterDate { committer_date },
                    })
                }
                ("modify", [name, path, rest @ .., content]) if rest.len() < 2 => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let file_type = match rest.get(0) {
                        Some(file_type) => file_type.to_string()?.parse()?,
                        None => FileType::Regular,
                    };
                    let content = content.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Modify {
                            path,
                            file_type,
                            content,
                        },
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
                ("copy", [name, path, rest @ .., content, parent, parent_path])
                    if rest.len() < 2 =>
                {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let file_type = match rest.get(0) {
                        Some(file_type) => file_type.to_string()?.parse()?,
                        None => FileType::Regular,
                    };
                    let content = content.to_bytes();
                    let parent = parent.to_string()?;
                    let parent_path = parent_path.to_bytes();
                    Ok(Action::Change {
                        name,
                        change: ChangeAction::Copy {
                            path,
                            file_type,
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
        let mut iter = args.trim().chars().peekable();
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
                    ch if ch.is_alphanumeric() || "_-./".contains(ch) => {
                        arg.push(ch);
                    }
                    '&' => {
                        while iter.peek().map_or(false, |ch| !ch.is_whitespace()) {
                            arg.push_hex(&mut iter)?;
                        }
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
    let ctx = app.new_basic_context();

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
    let mut bookmarks: BTreeMap<BookmarkKey, String> = BTreeMap::new();

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
        let dag: HashMap<_, _> = dag
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
            ChangeAction::Modify {
                path,
                file_type,
                content,
                ..
            } => c = c.add_file_with_type(path.as_slice(), content, file_type),
            ChangeAction::Delete { path, .. } => c = c.delete_file(path.as_slice()),
            ChangeAction::Forget { path, .. } => c = c.forget_file(path.as_slice()),
            ChangeAction::Extra { key, value, .. } => c = c.add_extra(key, value),
            ChangeAction::Message { message } => c = c.set_message(message),
            ChangeAction::Author { author } => c = c.set_author(author),
            ChangeAction::AuthorDate { author_date } => c = c.set_author_date(author_date),
            ChangeAction::Committer { committer } => c = c.set_committer(committer),
            ChangeAction::CommitterDate { committer_date } => {
                c = c.set_committer_date(committer_date)
            }
            ChangeAction::Copy {
                path,
                content,
                parent,
                parent_path,
                file_type,
                ..
            } => {
                let parent: CommitIdentifier =
                    committed.get(&parent).map_or(parent.into(), |&c| c.into());
                c = c.add_file_with_copy_info_and_type(
                    path.as_slice(),
                    content,
                    (parent, parent_path.as_slice()),
                    file_type,
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
    mgr.derive_exactly_batch::<D>(
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
        derive::<RootBasenameSuffixSkeletonManifest>(ctx, repo, csids),
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
                    file_type: FileType::Regular,
                    content: b"this has \xaa content\n\ton \x02 lines with \"quotes\"".to_vec(),
                }
            }
        );
        assert_eq!(
            Action::new("modify: _1 path/to/binary/file exec &Faceb00c")?,
            Action::Change {
                name: "_1".to_string(),
                change: ChangeAction::Modify {
                    path: b"path/to/binary/file".to_vec(),
                    file_type: FileType::Executable,
                    content: b"\xfa\xce\xb0\x0c".to_vec(),
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

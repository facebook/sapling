/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileType;
use mononoke_types::GitLfs;

use crate::CommitIdentifier;
use crate::CreateCommitContext;
use crate::Repo;

/// An action that affects the graph.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Action {
    /// Set whether default files are created (default: true).
    DefaultFiles { enabled: bool },
    /// Set a known changeset id for an already-existing commit.  This commit
    /// will not be created, but other commits in the graph that relate to it
    /// will be related to this existing commit.
    ///
    /// ```text
    ///     # exists: COMMIT id
    /// ```
    Exists { name: String, id: ChangesetId },
    /// Set a bookmark on a commit
    ///
    /// ```text
    ///     # bookmark: COMMIT name
    /// ```
    Bookmark { name: String, bookmark: BookmarkKey },
    /// Change a commit.  See ChangeAction for details.
    Change { name: String, change: ChangeAction },
    /// Change all commits in the same way.  Specify name as "*".   See ChangeAction for details.
    /// All-commit changes are applied before per-commit changes.
    ChangeAll { change: ChangeAction },
}

impl Action {
    fn make_change(name: String, change: ChangeAction) -> Self {
        if name == "*" {
            Action::ChangeAll { change }
        } else {
            Action::Change { name, change }
        }
    }
}

/// An action that changes one (or all) of the commits in the graph.  If the commit name is specified as "*",
/// then the action is applied to all commits before the commit-specific actions.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ChangeAction {
    /// Set the content of a file (optionally with file type).
    ///
    /// ```text
    ///     # modify: COMMIT path/to/file [TYPE] [LFS] "content"
    /// ```
    Modify {
        path: Vec<u8>,
        file_type: FileType,
        git_lfs: GitLfs,
        content: Vec<u8>,
    },
    /// Mark a file as deleted.
    ///
    /// ```text
    ///     # delete: COMMIT path/to/file
    /// ```
    Delete { path: Vec<u8> },
    /// Forget file that was about to be added (useful for getting rid of files
    /// that are added by default).
    ///
    /// ```text
    ///     # forget: COMMIT path/to/file
    /// ```
    Forget { path: Vec<u8> },
    /// Mark a file as a copy of another file (optionally with file type).
    ///
    /// ```text
    ///     # copy: COMMIT path/to/file [TYPE] [LFS] "content" PARENT_COMMIT_ID path/copied/from
    /// ```
    Copy {
        path: Vec<u8>,
        file_type: FileType,
        git_lfs: GitLfs,
        content: Vec<u8>,
        parent: String,
        parent_path: Vec<u8>,
    },
    /// Set a Mercurial commit extra on a commit.
    ///
    /// ```text
    ///     # extra: COMMIT "key" "value"
    /// ```
    Extra { key: String, value: Vec<u8> },
    /// Set the commit message.
    ///
    /// ```text
    ///     # message: COMMIT "message"
    /// ```
    Message { message: String },
    /// Set the author.
    ///
    /// ```text
    ///     # author: COMMIT "Author Name <email@domain>"
    /// ```
    Author { author: String },
    /// Set the author date (in RFC3339 format).
    ///
    /// ```text
    ///     # author_date: COMMIT "YYYY-mm-ddTHH:MM:SS+ZZ:ZZ"
    /// ```
    AuthorDate { author_date: DateTime },
    /// Set the committer.
    ///
    /// ```text
    ///     # committer: COMMIT "Committer Name <email@domain>"
    /// ```
    Committer { committer: String },
    /// Set the committer date (in RFC3339 format).
    ///
    /// ```text
    ///     # committer_date: COMMIT "YYYY-mm-ddTHH:MM:SS+ZZ:ZZ"
    /// ```
    CommitterDate { committer_date: DateTime },
}

impl Action {
    fn new(spec: &str) -> Result<Self> {
        if let Some((key, args)) = spec.trim().split_once(':') {
            let args = ActionArg::parse_args(args)
                .with_context(|| format!("Failed to parse args for '{}'", key))?;
            match (key, args.as_slice()) {
                ("default_files", [enabled]) => Ok(Action::DefaultFiles {
                    enabled: enabled.to_string()?.parse()?,
                }),
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
                    Ok(Action::make_change(name, ChangeAction::Message { message }))
                }
                ("author", [name, author]) => {
                    let name = name.to_string()?;
                    let author = author.to_string()?;
                    Ok(Action::make_change(name, ChangeAction::Author { author }))
                }
                ("author_date", [name, author_date]) => {
                    let name = name.to_string()?;
                    let author_date = DateTime::from_rfc3339(&author_date.to_string()?)?;
                    Ok(Action::make_change(
                        name,
                        ChangeAction::AuthorDate { author_date },
                    ))
                }
                ("committer", [name, committer]) => {
                    let name = name.to_string()?;
                    let committer = committer.to_string()?;
                    Ok(Action::make_change(
                        name,
                        ChangeAction::Committer { committer },
                    ))
                }
                ("committer_date", [name, committer_date]) => {
                    let name = name.to_string()?;
                    let committer_date = DateTime::from_rfc3339(&committer_date.to_string()?)?;
                    Ok(Action::make_change(
                        name,
                        ChangeAction::CommitterDate { committer_date },
                    ))
                }
                ("modify", [name, path, rest @ .., content]) if rest.len() < 3 => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let file_type = match rest.first() {
                        Some(file_type) => file_type.to_string()?.parse()?,
                        None => FileType::Regular,
                    };
                    let git_lfs = match rest.get(1) {
                        Some(git_lfs) => git_lfs.to_string()?.parse()?,
                        None => GitLfs::FullContent,
                    };
                    let content = content.to_bytes();
                    Ok(Action::make_change(
                        name,
                        ChangeAction::Modify {
                            path,
                            file_type,
                            git_lfs,
                            content,
                        },
                    ))
                }
                ("delete", [name, path]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    Ok(Action::make_change(name, ChangeAction::Delete { path }))
                }
                ("forget", [name, path]) => {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    Ok(Action::make_change(name, ChangeAction::Forget { path }))
                }
                ("extra", [name, key, value]) => {
                    let name = name.to_string()?;
                    let key = key.to_string()?;
                    let value = value.to_bytes();
                    Ok(Action::make_change(
                        name,
                        ChangeAction::Extra { key, value },
                    ))
                }
                ("copy", [name, path, rest @ .., content, parent, parent_path])
                    if rest.len() < 3 =>
                {
                    let name = name.to_string()?;
                    let path = path.to_bytes();
                    let file_type = match rest.first() {
                        Some(file_type) => file_type.to_string()?.parse()?,
                        None => FileType::Regular,
                    };
                    let git_lfs = match rest.get(1) {
                        Some(git_lfs) => git_lfs.to_string()?.parse()?,
                        None => GitLfs::FullContent,
                    };
                    let content = content.to_bytes();
                    let parent = parent.to_string()?;
                    let parent_path = parent_path.to_bytes();
                    Ok(Action::make_change(
                        name,
                        ChangeAction::Copy {
                            path,
                            file_type,
                            git_lfs,
                            content,
                            parent,
                            parent_path,
                        },
                    ))
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
                    ch if ch.is_alphanumeric()
                        || "_-./".contains(ch)
                        || (ch == '*' && arg.is_empty()) =>
                    {
                        arg.push(ch);
                    }
                    '&' => {
                        while iter.peek().is_some_and(|ch| !ch.is_whitespace()) {
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

fn apply_changes<'a, R: Repo>(
    mut c: CreateCommitContext<'a, R>,
    committed: &'_ BTreeMap<String, ChangesetId>,
    changes: Vec<ChangeAction>,
) -> CreateCommitContext<'a, R> {
    for change in changes {
        match change {
            ChangeAction::Modify {
                path,
                file_type,
                git_lfs,
                content,
                ..
            } => c = c.add_file_with_type_and_lfs(path.as_slice(), content, file_type, git_lfs),
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
                git_lfs,
                ..
            } => {
                let parent: CommitIdentifier =
                    committed.get(&parent).map_or(parent.into(), |&c| c.into());
                c = c.add_file_with_copy_info_and_type(
                    path.as_slice(),
                    content,
                    (parent, parent_path.as_slice()),
                    file_type,
                    git_lfs,
                )
            }
        }
    }
    c
}

pub async fn extend_from_dag_with_actions<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    dag: &'a str,
) -> Result<(
    BTreeMap<String, ChangesetId>,
    BTreeMap<String, BTreeSet<String>>,
)> {
    let mut dag_buffer = String::new();
    let mut actions = Vec::new();
    for line in dag.lines() {
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
    let mut all_commit_changes = Vec::new();
    let mut bookmarks: BTreeMap<BookmarkKey, String> = BTreeMap::new();
    let mut default_files = true;

    for action in actions {
        match action {
            Action::Exists { name, id } => {
                existing.insert(name, id);
            }
            Action::Bookmark { name, bookmark } => {
                bookmarks.insert(bookmark, name);
            }
            Action::DefaultFiles { enabled } => {
                default_files = enabled;
            }
            Action::Change { name, change } => {
                commit_changes
                    .entry(name)
                    .or_insert_with(Vec::new)
                    .push(change);
            }
            Action::ChangeAll { change } => {
                all_commit_changes.push(change);
            }
        }
    }

    let mut change_fns = BTreeMap::new();
    for (name, changes) in commit_changes {
        let apply: Box<ChangeFn<R>> = Box::new(
            move |c: CreateCommitContext<R>, committed: &'_ BTreeMap<String, ChangesetId>| {
                apply_changes(c, committed, changes.clone())
            },
        );
        change_fns.insert(name, apply);
    }

    let all_changes_fn: Option<Box<ChangeFn<R>>> = Some(Box::new(
        move |c: CreateCommitContext<R>, committed: &'_ BTreeMap<String, ChangesetId>| {
            apply_changes(c, committed, all_commit_changes.clone())
        },
    ));

    let (commits, dag) = extend_from_dag_with_changes(
        ctx,
        repo,
        &dag_buffer,
        all_changes_fn,
        change_fns,
        existing,
        default_files,
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
                .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
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

    Ok((commits, dag))
}

pub type ChangeFn<R> = dyn for<'a, 'b> FnMut(
        CreateCommitContext<'a, R>,
        &'b BTreeMap<String, ChangesetId>,
    ) -> CreateCommitContext<'a, R>
    + Send
    + Sync;

pub async fn extend_from_dag_with_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    dag: &'a str,
    mut all_changes: Option<Box<ChangeFn<R>>>,
    mut changes: BTreeMap<String, Box<ChangeFn<R>>>,
    existing: BTreeMap<String, ChangesetId>,
    default_files: bool,
) -> Result<(
    BTreeMap<String, ChangesetId>,
    BTreeMap<String, BTreeSet<String>>,
)> {
    let mut committed: BTreeMap<String, ChangesetId> = BTreeMap::new();
    let dag = drawdag::parse(dag);

    for (name, id) in existing {
        if !dag.contains_key(&name) {
            return Err(anyhow!("graph does not contain {}", name));
        }
        committed.insert(name, id);
    }

    while committed.len() < dag.len() {
        let mut made_progress = false;
        for (name, parents) in dag.iter() {
            if committed.contains_key(name) {
                // This node was already committed.
                continue;
            }

            if parents.iter().any(|parent| !committed.contains_key(parent)) {
                // This node still has uncommitted parents.
                continue;
            }

            let parent_ids = parents
                .iter()
                .map(|parent| committed[parent].clone())
                .collect();
            let mut create_commit =
                CreateCommitContext::new(ctx, repo, parent_ids).set_message(name);

            if default_files {
                create_commit = create_commit.add_file(name.as_str(), name.as_str());
            }
            if let Some(change) = all_changes.as_mut() {
                create_commit = change(create_commit, &committed);
            }
            if let Some(mut change) = changes.remove(name.as_str()) {
                create_commit = change(create_commit, &committed);
            }
            let new_id = create_commit.commit().await?;
            committed.insert(name.to_string(), new_id);
            made_progress = true;
        }
        if !made_progress {
            return Err(anyhow!("graph contains cycles"));
        }
    }

    Ok((committed, dag))
}

/// Create commits from an ASCII DAG.
///
/// Creates a set of commits that correspond to an ASCII DAG, with
/// customizable changes.
///
/// By default, each commit will have the commit message set to the name of
/// the commit, and will have a single file added in the root with that name
/// and content.  For example, commit `B` will contain the addition of file
/// `B` with content `B`.
///
/// The contents of each commit can be customized by a closure.  Each
/// closure takes the `CreateCommitContext` and should return that
/// context after customization.  The context will already have the
/// commit message set, and the default file mentioned above for that
/// commit already added (so commit `B` will have file `B` already).
/// Use `forget_file(NAME)` to remove it from the set of file changes if
/// this is not wanted (see commit `C` in the example below).
///
/// Use the `changes!` macro to generate the map of customization closures.
///
/// DAGs can be anything parseable by the `drawdag` crate, and can be
/// either horizontal (left-to-right) or vertical (bottom-to-top).
///
/// Example:
///
/// ```ignore
///     create_from_dag(
///         ctx,
///         repo,
///         r##"
///             A-B-C
///                \
///                 D
///         "##,
///         changes! {
///            "B" => |c| c.set_author("test"),
///            "C" => |c| c.forget_file("C").delete_file("A"),
///         },
///     ).await?;
/// ```
pub async fn create_from_dag_with_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    dag: &'a str,
    changes: BTreeMap<String, Box<ChangeFn<R>>>,
) -> Result<BTreeMap<String, ChangesetId>> {
    let (commits, _dag) =
        extend_from_dag_with_changes(ctx, repo, dag, None, changes, BTreeMap::new(), true).await?;
    Ok(commits)
}

/// Create commits from an ASCII DAG.
///
/// Creates a set of commits that correspond to an ASCII DAG.  Each
/// commit will have the commit message set to the name of the commit,
/// and will have a single file added in the root with that name and
/// content. For example, commit `B` will contain the addition of
/// file `B` with content `B`.
///
/// DAGs can be anything parseable by the `drawdag` crate, and can be
/// either horizontal (left-to-right) or vertical (bottom-to-top).
///
/// Example:
///
/// ```ignore
///     create_from_dag(
///         ctx,
///         repo,
///         r##"
///             A-B-C
///                \
///                 D
///         "##,
///     ).await?;
/// ```
pub async fn create_from_dag<R: Repo>(
    ctx: &CoreContext,
    repo: &R,
    dag: &str,
) -> Result<BTreeMap<String, ChangesetId>> {
    create_from_dag_with_changes(ctx, repo, dag, BTreeMap::new()).await
}

/// Macro to allow creation of `changes` for `create_from_dag_with_changes`.
///
/// Example:
///
/// ```ignore
///     create_from_dag_with_changes(
///         ctx,
///         repo,
///         "A-B-C-D",
///         changes! {
///             "B" => |c| c.set_author("test"),
///             "C" => |c| c.delete_file("A"),
///             "D" => |c, commits| c.add_file_with_copy_info("file", "content", (*commits.get("C").unwrap(), "orig"))
///         }
///     ).await?;
/// ```
#[macro_export]
macro_rules! __drawdag_changes {
    ( $( $key:expr => | $c:ident | $body:expr ),* $( , )? ) => {
        {
            let mut changes: std::collections::BTreeMap<String, Box<$crate::drawdag::ChangeFn<_>>> =
                std::collections::BTreeMap::new();
            $(
                changes.insert(String::from($key), Box::new(move |$c: $crate::CreateCommitContext<_>, _: &'_ std::collections::BTreeMap<String, ::mononoke_types::ChangesetId>| $body));
            )*
            changes
        }
    };
    ( $( $key:expr => | $c:ident, $d: ident | $body:expr ),* $( , )? ) => {
        {
            let mut changes: std::collections::BTreeMap<String, Box<$crate::drawdag::ChangeFn<_>>> =
                std::collections::BTreeMap::new();
            $(
                changes.insert(String::from($key), Box::new(move |$c: $crate::CreateCommitContext<_>, $d: &'_ std::collections::BTreeMap<String, ::mononoke_types::ChangesetId>| $body));
            )*
            changes
        }
    };
}

// Export macro within this module.
pub use __drawdag_changes as changes;

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
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
                    git_lfs: GitLfs::FullContent,
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
                    git_lfs: GitLfs::FullContent,
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
        assert_eq!(
            Action::new("author_date: * \"2024-02-29T13:37:00Z\"")?,
            Action::ChangeAll {
                change: ChangeAction::AuthorDate {
                    author_date: DateTime::from_rfc3339("2024-02-29T13:37:00Z")?,
                }
            }
        );
        Ok(())
    }
}

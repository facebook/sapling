/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::CreateCommitContext;
use crate::Repo;

pub type ChangeFn<R> = dyn for<'a, 'b> FnOnce(
        CreateCommitContext<'a, R>,
        &'b BTreeMap<String, ChangesetId>,
    ) -> CreateCommitContext<'a, R>
    + Send
    + Sync;

pub async fn extend_from_dag_with_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    dag: &'a str,
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
            if let Some(change) = changes.remove(name.as_str()) {
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
        extend_from_dag_with_changes(ctx, repo, dag, changes, BTreeMap::new(), true).await?;
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
                changes.insert(String::from($key), Box::new(|$c: $crate::CreateCommitContext<_>, _: &'_ std::collections::BTreeMap<String, ::mononoke_types::ChangesetId>| $body));
            )*
            changes
        }
    };
    ( $( $key:expr => | $c:ident, $d: ident | $body:expr ),* $( , )? ) => {
        {
            let mut changes: std::collections::BTreeMap<String, Box<$crate::drawdag::ChangeFn<_>>> =
                std::collections::BTreeMap::new();
            $(
                changes.insert(String::from($key), Box::new(|$c: $crate::CreateCommitContext<_>, $d: &'_ std::collections::BTreeMap<String, ::mononoke_types::ChangesetId>| $body));
            )*
            changes
        }
    };
}

// Export macro within this module.
pub use __drawdag_changes as changes;

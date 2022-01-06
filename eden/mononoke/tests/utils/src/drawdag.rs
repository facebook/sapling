/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Result;
use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::CreateCommitContext;

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
pub async fn create_from_dag_with_changes<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    dag: &'a str,
    mut changes: BTreeMap<&'a str, Box<dyn FnMut(CreateCommitContext) -> CreateCommitContext>>,
) -> Result<BTreeMap<String, ChangesetId>> {
    let mut committed: BTreeMap<String, ChangesetId> = BTreeMap::new();

    let dag = drawdag::parse(dag);

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
            let mut create_commit = CreateCommitContext::new(ctx, repo, parent_ids)
                .set_message(name)
                .add_file(name.as_str(), name);
            if let Some(change) = changes.get_mut(name.as_str()) {
                create_commit = change(create_commit);
            }
            let new_id = create_commit.commit().await?;
            committed.insert(name.to_string(), new_id);
            made_progress = true;
        }
        assert!(made_progress, "graph contains cycles");
    }

    Ok(committed)
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
pub async fn create_from_dag(
    ctx: &CoreContext,
    repo: &BlobRepo,
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
///         "A-B-C",
///         changes! {
///             "B" => |c| c.set_author("test"),
///             "C" => |c| c.delete_file("A"),
///         }
///     ).await?;
/// ```
#[macro_export]
macro_rules! __drawdag_changes {
    ( $( $key:expr => | $c:ident | $body:expr ),* $( , )? ) => {
        {
            type ChangeFn =
                dyn FnMut($crate::CreateCommitContext) -> $crate::CreateCommitContext;
            let mut changes: std::collections::BTreeMap<&str, Box<ChangeFn>> =
                std::collections::BTreeMap::new();
            $(
                changes.insert($key, Box::new(|$c: $crate::CreateCommitContext| $body));
            )*
            changes
        }
    };
}

// Export macro within this module.
pub use __drawdag_changes as changes;

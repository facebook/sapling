/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use commit_id_types::CommitIdsArgs;
use maplit::btreeset;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit_id::render_commit_id;
use crate::render::Render;

#[derive(clap::Parser)]
/// Rebase a stack of draft commits onto a commit without moving a bookmark
///
/// Provide three commits: the head of the stack, the commit the stack is
/// based on, and the destination. The destination may be older than, newer
/// than, or unrelated to the base.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
    /// Treat any path changed on both sides as a conflict instead of
    /// content-merging it
    #[clap(long)]
    no_merge: bool,
}

#[derive(Serialize)]
struct RebasedCommit {
    old_bonsai_id: String,
    new_ids: BTreeMap<String, String>,
    merged_paths: Vec<String>,
    dropped: bool,
}

#[derive(Serialize)]
struct RebaseStackOutput {
    head: BTreeMap<String, String>,
    rebased_commits: Vec<RebasedCommit>,
    overlapping_path_count: i64,
    merged_path_count: i64,
}

impl Render for RebaseStackOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_string_set();
        write!(w, "head")?;
        render_commit_id(Some(("", "    ")), "\n", "head", &self.head, &schemes, w)?;
        write!(w, "\n")?;
        for rebase in self.rebased_commits.iter() {
            write!(w, "{} => ", rebase.old_bonsai_id)?;
            if rebase.dropped {
                write!(w, "dropped")?;
            } else {
                render_commit_id(None, ", ", "new commit", &rebase.new_ids, &schemes, w)?;
            }
            if !rebase.merged_paths.is_empty() {
                write!(w, " (merged: {})", rebase.merged_paths.join(", "))?;
            }
            write!(w, "\n")?;
        }
        if self.overlapping_path_count > 0 {
            write!(
                w,
                "{} overlapping paths, {} merged\n",
                self.overlapping_path_count, self.merged_path_count
            )?;
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.into_repo_specifier();
    let commit_ids = args.commit_ids_args.into_commit_ids();
    if commit_ids.len() != 3 {
        bail!(
            "expected 3 commit_ids: head, base, onto (got {})",
            commit_ids.len()
        )
    }
    let conn = app.get_connection(Some(&repo.name)).await?;
    let ids = resolve_commit_ids(&conn, &repo, &commit_ids).await?;
    let (head, base, onto) = match ids.as_slice() {
        [head, base, onto] => (head.clone(), base.clone(), onto.clone()),
        _ => bail!("expected 3 commit_ids (got {})", ids.len()),
    };

    let params = thrift::RepoRebaseStackParams {
        head,
        base,
        onto,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        old_identity_schemes: Some(btreeset! { thrift::CommitIdentityScheme::BONSAI }),
        merge_resolution: if args.no_merge {
            thrift::RepoRebaseStackMergeResolution::DISABLED
        } else {
            thrift::RepoRebaseStackMergeResolution::DEFAULT
        },
        service_identity: args.service_id_args.service_id,
        ..Default::default()
    };
    let response = conn
        .repo_rebase_stack(&repo, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    let rebased_commits = response
        .rebased_commits
        .into_iter()
        .map(|rebase| {
            let (_, old_bonsai_id) = map_commit_id(
                rebase
                    .old_ids
                    .get(&thrift::CommitIdentityScheme::BONSAI)
                    .ok_or_else(|| anyhow!("bonsai id missing from response"))?,
            )
            .ok_or_else(|| anyhow!("bonsai id should be mappable"))?;
            Ok(RebasedCommit {
                old_bonsai_id,
                new_ids: map_commit_ids(rebase.new_ids.values()),
                merged_paths: rebase.merged_paths,
                dropped: rebase.dropped,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let output = RebaseStackOutput {
        head: map_commit_ids(response.head.values()),
        rebased_commits,
        overlapping_path_count: response.overlapping_path_count,
        merged_path_count: response.merged_path_count,
    };
    app.target.render_one(&args.scheme_args, output).await
}

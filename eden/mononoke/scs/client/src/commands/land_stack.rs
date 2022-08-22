/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use maplit::btreeset;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]

/// Land a stack of commits
///
/// Provide two commits: the first is the head of a stack, and the second is
/// public commit the stack is based on.  The stack of commits between these
/// two commits will be landed onto the named bookmark via pushrebase.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
    #[clap(flatten)]
    pushvar_args: PushvarArgs,
    #[clap(long, short)]
    /// Name of the bookmark to land to
    name: String,
}

#[derive(Serialize)]
struct PushrebaseRebasedCommit {
    old_bonsai_id: String,
    new_ids: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct PushrebaseOutcomeOutput {
    bookmark: String,
    head: BTreeMap<String, String>,
    rebased_commits: Vec<PushrebaseRebasedCommit>,
    retries: i64,
    distance: i64,
}

impl Render for PushrebaseOutcomeOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_string_set();
        write!(
            w,
            "In {} retries across distance {}\n",
            self.retries, self.distance
        )?;
        write!(w, "{} updated to", self.bookmark)?;
        render_commit_id(
            Some(("", "    ")),
            "\n",
            &self.bookmark,
            &self.head,
            &schemes,
            w,
        )?;
        write!(w, "\n")?;
        for rebase in self.rebased_commits.iter() {
            write!(w, "{} => ", rebase.old_bonsai_id)?;
            render_commit_id(None, ", ", "new commit", &rebase.new_ids, &schemes, w)?;
            write!(w, "\n")?;
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
    if commit_ids.len() != 2 {
        bail!("expected 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
    let bookmark = args.name;
    let service_identity = args.service_id_args.service_id;
    let pushvars = args.pushvar_args.into_pushvars();

    let (head, base) = match ids.as_slice() {
        [head_id, base_id] => (head_id.clone(), base_id.clone()),
        _ => bail!("expected 1 or 2 commit_ids (got {})", ids.len()),
    };

    let params = thrift::RepoLandStackParams {
        bookmark: bookmark.clone(),
        head,
        base,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        old_identity_schemes: Some(btreeset! { thrift::CommitIdentityScheme::BONSAI }),
        service_identity,
        pushvars,
        ..Default::default()
    };
    let outcome = app
        .connection
        .repo_land_stack(&repo, &params)
        .await?
        .pushrebase_outcome;
    let head = map_commit_ids(outcome.head.values());
    let mut rebased_commits = outcome
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
            let new_ids = map_commit_ids(rebase.new_ids.values());
            Ok(PushrebaseRebasedCommit {
                old_bonsai_id,
                new_ids,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    rebased_commits.sort_unstable_by(|a, b| a.old_bonsai_id.cmp(&b.old_bonsai_id));
    let output = PushrebaseOutcomeOutput {
        bookmark,
        head,
        rebased_commits,
        distance: outcome.pushrebase_distance,
        retries: outcome.retry_num,
    };
    app.target.render_one(&args.scheme_args, output).await
}

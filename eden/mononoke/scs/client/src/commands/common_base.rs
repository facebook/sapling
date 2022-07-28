/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Find common base of two commits

use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;
use std::collections::BTreeMap;
use std::io::Write;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Finds a common base of two commits.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
}

#[derive(Serialize)]
struct CommonBaseOutput {
    #[serde(skip)]
    pub requested: (String, String),
    pub exists: bool,
    pub ids: BTreeMap<String, String>,
}

impl Render for CommonBaseOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            let schemes = args.scheme_args.scheme_string_set();
            render_commit_id(None, "\n", "common base", &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!(
                "a common ancestor of {} and {} does not exist\n",
                self.requested.0,
                self.requested.1
            );
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    let ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
    let ids = match ids.as_slice() {
        [id0, id1] => (id0.clone(), id1.clone()),
        _ => bail!("expected 2 commit_ids (got {})", commit_ids.len()),
    };
    let commit = thrift::CommitSpecifier {
        repo,
        id: ids.0,
        ..Default::default()
    };
    let params = thrift::CommitCommonBaseWithParams {
        other_commit_id: ids.1,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        ..Default::default()
    };
    let response = app
        .connection
        .commit_common_base_with(&commit, &params)
        .await?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };
    let output = CommonBaseOutput {
        requested: (commit_ids[0].to_string(), commit_ids[1].to_string()),
        exists: response.exists,
        ids,
    };
    app.target.render_one(&args, output).await
}

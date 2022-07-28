/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Look up a bookmark or commit id.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Look up a bookmark or commit id
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
}

#[derive(Serialize)]
pub(crate) struct LookupOutput {
    #[serde(skip)]
    pub requested: String,
    pub exists: bool,
    pub ids: BTreeMap<String, String>,
}

impl Render for LookupOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            let schemes = args.scheme_string_set();
            render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!("{} does not exist\n", self.requested);
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitLookupParams {
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        ..Default::default()
    };
    let response = app.connection.commit_lookup(&commit, &params).await?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };
    let output = LookupOutput {
        requested: commit_id.to_string(),
        exists: response.exists,
        ids,
    };
    app.target.render_one(&args.scheme_args, output).await
}

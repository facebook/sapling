/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use commit_id_types::CommitIdArgs;
use faster_hex::hex_string;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

/// Fingerprint algorithm version.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Serialize)]
enum FingerprintVersion {
    /// V1: root FsnodeId blake2 hash (universally available)
    #[clap(name = "1")]
    V1,
    /// V2: root ContentManifestId blake2 hash (recommended; requires
    /// ContentManifests derived data enabled for the repo)
    #[clap(name = "2")]
    V2,
}

impl From<FingerprintVersion> for thrift::CommitFingerprintVersion {
    fn from(v: FingerprintVersion) -> Self {
        match v {
            FingerprintVersion::V1 => thrift::CommitFingerprintVersion::V1,
            FingerprintVersion::V2 => thrift::CommitFingerprintVersion::V2,
        }
    }
}

/// Get the content fingerprint of a commit.
#[derive(clap::Parser)]
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    /// Fingerprint algorithm version
    #[clap(long, value_enum, default_value = "1")]
    version: FingerprintVersion,
}

#[derive(Serialize)]
struct CommitFingerprintOutput {
    fingerprint: String,
    version: FingerprintVersion,
}

impl Render for CommitFingerprintOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "{}", self.fingerprint)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name)).await?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitFingerprintParams {
        version: args.version.into(),
        ..Default::default()
    };
    let response = conn
        .commit_fingerprint(&commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;
    let output = CommitFingerprintOutput {
        fingerprint: hex_string(&response.fingerprint),
        version: args.version,
    };
    app.target.render_one(&args, output).await
}

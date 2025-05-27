/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Fetch the contents of a file.

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use cloned::cloned;
use commit_id_types::CommitIdArgs;
use futures::TryFutureExt;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use scs_client_raw::thrift;
use serde_json::json;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::path::PathArgs;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

/// Chunk size for requests.
const CHUNK_SIZE: i64 = source_control::FILE_CONTENT_CHUNK_RECOMMENDED_SIZE;

/// Number of concurrent fetches for very large files.
const CONCURRENT_FETCHES: usize = 10;

#[derive(Parser)]
/// Fetch the contents of a file
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    path_args: PathArgs,
}

struct CatOutput {
    offset: u64,
    data: Vec<u8>,
}

impl Render for CatOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        w.write_all(self.data.as_slice())?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let output = match std::str::from_utf8(self.data.as_slice()) {
            Ok(data) => json!({
                "offset": self.offset,
                "data": data,
            }),
            Err(_) => json!({
                "offset": self.offset,
                "hex": faster_hex::hex_string(self.data.as_slice())
            }),
        };
        Ok(serde_json::to_writer(w, &output)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let conn = app.get_connection(Some(&repo.name))?;
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let path = args.path_args.path.clone();
    let file = thrift::FileSpecifier::by_commit_path(thrift::CommitPathSpecifier {
        commit,
        path,
        ..Default::default()
    });

    // Request the first chunk of the file.
    let params = thrift::FileContentChunkParams {
        offset: 0,
        size: CHUNK_SIZE,
        ..Default::default()
    };
    let response = conn
        .file_content_chunk(&file, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    let output = CatOutput {
        offset: response.offset as u64,
        data: response.data,
    };

    let file_size = response.file_size;
    let stream = stream::once(future::ok(output)).chain(
        stream::iter((CHUNK_SIZE..file_size).step_by(CHUNK_SIZE as usize))
            .map(move |offset| {
                cloned!(repo);
                let params = thrift::FileContentChunkParams {
                    offset,
                    size: CHUNK_SIZE,
                    ..Default::default()
                };
                conn.file_content_chunk(&file, &params)
                    .map_err(move |e| e.handle_selection_error(&repo.clone()))
            })
            .buffered(CONCURRENT_FETCHES)
            .then(|response| {
                future::ready(response.map(|response| CatOutput {
                    offset: response.offset as u64,
                    data: response.data,
                }))
            }),
    );
    app.target.render(&args, stream).await
}

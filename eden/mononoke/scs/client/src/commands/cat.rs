/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Fetch the contents of a file.

use std::io::Write;

use anyhow::Error;
use clap::App;
use clap::AppSettings;
use clap::ArgMatches;
use clap::SubCommand;
use futures::future;
use futures::stream;
use futures::TryFutureExt;
use futures_util::stream::StreamExt;
use serde_json::json;
use source_control::types as thrift;

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::resolve_commit_id;
use crate::args::path::add_path_args;
use crate::args::path::get_path;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "cat";

/// Chunk size for requests.
const CHUNK_SIZE: i64 = source_control::FILE_CONTENT_CHUNK_RECOMMENDED_SIZE;

/// Number of concurrent fetches for very large files.
const CONCURRENT_FETCHES: usize = 10;

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Fetch the contents of a file")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_path_args(cmd);
    cmd
}

struct CatOutput {
    offset: u64,
    data: Vec<u8>,
}

impl Render for CatOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        w.write_all(self.data.as_slice())?;
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
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

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let path = get_path(matches).expect("path is required");
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
    let response = connection.file_content_chunk(&file, &params).await?;
    let output = Box::new(CatOutput {
        offset: response.offset as u64,
        data: response.data,
    });

    let file_size = response.file_size;
    let stream = stream::once(async move { Ok(output as Box<dyn Render>) }).chain(
        stream::iter((CHUNK_SIZE..file_size).step_by(CHUNK_SIZE as usize))
            .map(move |offset| {
                let params = thrift::FileContentChunkParams {
                    offset,
                    size: CHUNK_SIZE,
                    ..Default::default()
                };
                connection
                    .file_content_chunk(&file, &params)
                    .map_err(Error::from)
            })
            .buffered(CONCURRENT_FETCHES)
            .then(|response| {
                future::ready(response.map(|response| {
                    let output = Box::new(CatOutput {
                        offset: response.offset as u64,
                        data: response.data,
                    });
                    output as Box<dyn Render>
                }))
            }),
    );
    Ok(stream.boxed())
}

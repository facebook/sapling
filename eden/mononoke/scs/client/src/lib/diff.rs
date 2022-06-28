/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Helper library for returning file diffs

use std::io::Write;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use futures_util::stream;
use futures_util::stream::StreamExt;
use serde_derive::Serialize;
use source_control as thrift;

use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

#[derive(Serialize)]
struct DiffOutput {
    diffs: Vec<Vec<u8>>,
}

impl Render for DiffOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        for diff in &self.diffs {
            write!(w, "{}", String::from_utf8_lossy(diff))?;
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

async fn make_file_diff_request(
    connection: &Connection,
    commit: &thrift::CommitSpecifier,
    other_commit_id: Option<thrift::CommitId>,
    paths: Vec<thrift::CommitFileDiffsParamsPathPair>,
) -> Result<Box<DiffOutput>, Error> {
    let params = thrift::CommitFileDiffsParams {
        other_commit_id,
        paths,
        format: thrift::DiffFormat::RAW_DIFF,
        context: 3,
        ..Default::default()
    };

    let response = connection.commit_file_diffs(&commit, &params).await?;
    let diffs: Vec<_> = response
        .path_diffs
        .into_iter()
        .filter_map(|path_diff| {
            if let thrift::Diff::raw_diff(diff) = path_diff.diff {
                Some(diff.raw_diff.unwrap_or(Vec::new()))
            } else {
                None
            }
        })
        .collect();

    Ok(Box::new(DiffOutput { diffs }))
}

/// Given the paths and sizes of files to diff returns the stream of renderable
/// structs. The sizes are used to avoid hitting size limit when doing batch requests.
pub(crate) fn diff_files<I>(
    connection: &Connection,
    commit: thrift::CommitSpecifier,
    other_commit_id: Option<thrift::CommitId>,
    paths_sizes: I,
) -> Result<RenderStream, Error>
where
    I: IntoIterator<Item = (thrift::CommitFileDiffsParamsPathPair, i64)>,
{
    let mut size_sum: i64 = 0;
    let mut path_count: i64 = 0;
    let mut paths = Vec::new();
    let mut requests = Vec::new();
    cloned!(connection);
    for (path, size) in paths_sizes {
        if size + size_sum > thrift::COMMIT_FILE_DIFFS_SIZE_LIMIT
            || path_count + 1 > thrift::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT
        {
            requests.push(paths);
            paths = Vec::new();
            size_sum = 0;
            path_count = 0;
        }
        paths.push(path);
        path_count += 1;
        size_sum += size;
    }
    requests.push(paths);
    Ok(stream::iter(requests)
        .then(move |paths| {
            let connection = connection.clone();
            let commit = commit.clone();
            let other_commit_id = other_commit_id.clone();
            async move {
                make_file_diff_request(&connection, &commit, other_commit_id, paths)
                    .await
                    .map(|d| d as Box<dyn Render>)
            }
        })
        .boxed())
}

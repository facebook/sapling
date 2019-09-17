// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg};
use cmdlib::args;
use context::CoreContext;
use failure::err_msg;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures_ext::BoxStream;
use futures_ext::FutureExt;
use manifest::ManifestOps;
use mercurial_types::{Changeset, FileBytes, HgChangesetId};
use mononoke_types::FileType;
use std::str::FromStr;
use std::sync::Arc;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
    };
    app.build("Tool to calculate repo statistic")
        .version("0.0.0")
        .arg(
            Arg::with_name("changeset")
                .long("changeset")
                .required(true)
                .takes_value(true)
                .value_name("CHANGESET")
                .help("hg changeset hash"),
        )
}

#[derive(Clone, Copy, Default)]
pub struct RepoStatistics {
    num_files: u64,
    total_file_size: u64,
    num_lines: u64,
}

impl RepoStatistics {
    pub fn new(num_files: u64, total_file_size: u64, num_lines: u64) -> Self {
        Self {
            num_files,
            total_file_size,
            num_lines,
        }
    }
}

pub fn number_of_lines(
    bytes_stream: BoxStream<FileBytes, Error>,
) -> impl Future<Item = u64, Error = Error> {
    bytes_stream
        .map(|bytes| {
            bytes.into_iter().fold(0, |num_lines, byte| {
                if byte == '\n' as u8 {
                    num_lines + 1
                } else {
                    num_lines
                }
            })
        })
        .fold(0, |result, num_lines| {
            future::ok::<_, Error>(result + num_lines)
        })
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches();

    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(&matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let changeset = matches
        .value_of("changeset")
        .ok_or(err_msg("required parameter `changeset` is not set"))?;
    let changeset = HgChangesetId::from_str(changeset)?;

    let repo_statistics = args::open_repo(fb, &logger, &matches).and_then(move |repo| {
        let blobstore = Arc::new(repo.get_blobstore());
        repo.get_changeset_by_changesetid(ctx.clone(), changeset.clone())
            .map(move |changeset| changeset.manifestid())
            .and_then(move |manifest_id| {
                manifest_id
                    .list_leaf_entries(ctx.clone(), blobstore.clone())
                    .map(move |(_, (file_type, filenode_id))| {
                        if file_type == FileType::Regular {
                            number_of_lines(repo.get_file_content(ctx.clone(), filenode_id))
                                .left_future()
                        } else {
                            future::ok(0).right_future()
                        }
                        .join(repo.get_file_size(ctx.clone(), filenode_id))
                    })
                    .buffered(100)
                    .fold(
                        RepoStatistics::default(),
                        |mut statistics, (lines, file_size)| {
                            statistics.num_files += 1;
                            statistics.total_file_size += file_size;
                            statistics.num_lines += lines;
                            future::ok::<_, Error>(statistics)
                        },
                    )
            })
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let repo_statistics = runtime.block_on(repo_statistics)?;
    runtime.shutdown_on_idle();

    println!(
        "Number of files: {}, total file size: {}, number of lines: {}",
        repo_statistics.num_files, repo_statistics.total_file_size, repo_statistics.num_lines
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::stream;
    use tokio::runtime::Runtime;

    #[test]
    fn test_number_of_lines_empty_stream() -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();

        let stream: BoxStream<FileBytes, Error> =
            Box::new(stream::once(Ok(FileBytes(Bytes::from(&b""[..])))));
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 0);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_one_line() -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();

        let stream: BoxStream<FileBytes, Error> = Box::new(stream::once(Ok(FileBytes(
            Bytes::from(&b"First line\n"[..]),
        ))));
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 1);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_many_lines() -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();

        let stream: BoxStream<FileBytes, Error> = Box::new(stream::once(Ok(FileBytes(
            Bytes::from(&b"First line\nSecond line\nThird line\n"[..]),
        ))));
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 3);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_many_items() -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();

        let vec = vec![
            FileBytes(Bytes::from(&b"First line\n"[..])),
            FileBytes(Bytes::from(&b""[..])),
            FileBytes(Bytes::from(&b"First line\nSecond line\nThird line\n"[..])),
        ];
        let stream: BoxStream<FileBytes, Error> = Box::new(stream::iter_ok(vec));
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 4);
        Ok(())
    }
}

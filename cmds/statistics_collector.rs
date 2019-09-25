// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bookmarks::BookmarkName;
use clap::{App, Arg};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure::err_msg;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::future;
use futures::future::Future;
use futures::future::{loop_fn, Loop};
use futures::stream::Stream;
use futures_ext::BoxStream;
use futures_ext::FutureExt;
use manifest::{Diff, Entry, ManifestOps};
use mercurial_types::{Changeset, FileBytes, HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::FileType;
//use scuba::{ScubaClient, ScubaSample};
use scuba_ext::ScubaSampleBuilder;
use std::ops::{Add, Sub};
use std::sync::Arc;
use std::time::Duration;

const SCUBA_DATASET_NAME: &str = "mononoke_repository_statistics";

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
    };
    app.build("Tool to calculate repo statistic")
        .version("0.0.0")
        .arg(
            Arg::with_name("bookmark")
                .long("bookmark")
                .takes_value(true)
                .required(false)
                .help("bookmark from which we get statistics"),
        )
        .arg(
            Arg::with_name("log-to-scuba")
                .long("log-to-scuba")
                .takes_value(false)
                .required(false)
                .help("if set then statistics are logged to scuba"),
        )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RepoStatistics {
    num_files: i64,
    total_file_size: i64,
    num_lines: i64,
}

impl RepoStatistics {
    pub fn new(num_files: i64, total_file_size: i64, num_lines: i64) -> Self {
        Self {
            num_files,
            total_file_size,
            num_lines,
        }
    }
}

impl Add for RepoStatistics {
    type Output = RepoStatistics;

    fn add(self, other: Self) -> Self {
        Self {
            num_files: self.num_files + other.num_files,
            total_file_size: self.total_file_size + other.total_file_size,
            num_lines: self.num_lines + other.num_lines,
        }
    }
}

impl Sub for RepoStatistics {
    type Output = RepoStatistics;

    fn sub(self, other: Self) -> Self {
        Self {
            num_files: self.num_files - other.num_files,
            total_file_size: self.total_file_size - other.total_file_size,
            num_lines: self.num_lines - other.num_lines,
        }
    }
}

pub fn number_of_lines(
    bytes_stream: BoxStream<FileBytes, Error>,
) -> impl Future<Item = i64, Error = Error> {
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

pub fn get_manifest_from_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    changeset: HgChangesetId,
) -> impl Future<Item = HgManifestId, Error = Error> {
    repo.get_changeset_by_changesetid(ctx.clone(), changeset.clone())
        .map(move |changeset| changeset.manifestid())
}

// Calculates number of lines only for regular-type file
pub fn get_statistics_from_entry(
    ctx: CoreContext,
    repo: BlobRepo,
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> impl Future<Item = RepoStatistics, Error = Error> {
    match entry {
        Entry::Leaf((file_type, filenode_id)) => match file_type {
            FileType::Regular => {
                number_of_lines(repo.get_file_content(ctx.clone(), filenode_id)).left_future()
            }
            _ => future::ok(0).right_future(),
        }
        .join(
            repo.get_file_size(ctx.clone(), filenode_id)
                .and_then(move |size| future::ok(size as i64)),
        )
        .left_future(),
        Entry::Tree(_) => future::ok((0, 0)).right_future(),
    }
    .map(move |(lines, size)| match entry {
        Entry::Leaf(_) => RepoStatistics::new(1, size, lines),
        Entry::Tree(_) => RepoStatistics::new(0, size, lines),
    })
}

pub fn get_statistics_from_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    blobstore: impl Blobstore + Clone,
    hg_cs_id: HgChangesetId,
) -> impl Future<Item = RepoStatistics, Error = Error> {
    get_manifest_from_changeset(ctx.clone(), repo.clone(), hg_cs_id.clone()).and_then({
        cloned!(ctx, repo);
        move |manifest_id| {
            manifest_id
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .map(move |(_, leaf)| {
                    get_statistics_from_entry(ctx.clone(), repo.clone(), Entry::Leaf(leaf))
                })
                .buffered(100)
                .fold(RepoStatistics::default(), |statistics, new_stat| {
                    future::ok::<_, Error>(statistics + new_stat)
                })
                .map(|statistics| statistics)
        }
    })
}

pub fn update_statistics(
    ctx: CoreContext,
    repo: BlobRepo,
    statistics: RepoStatistics,
    diff: BoxStream<Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>, Error>,
) -> impl Future<Item = RepoStatistics, Error = Error> {
    diff.map({
        move |diff| match diff {
            Diff::Added(_, entry) => {
                get_statistics_from_entry(ctx.clone(), repo.clone(), entry.clone())
                    .map(|stat| (stat, Operation::Add))
                    .boxify()
            }
            Diff::Removed(_, entry) => {
                get_statistics_from_entry(ctx.clone(), repo.clone(), entry.clone())
                    .map(|stat| (stat, Operation::Sub))
                    .boxify()
            }
            Diff::Changed(_, old_entry, new_entry) => {
                get_statistics_from_entry(ctx.clone(), repo.clone(), old_entry.clone())
                    .join(get_statistics_from_entry(
                        ctx.clone(),
                        repo.clone(),
                        new_entry.clone(),
                    ))
                    .map(|(old_stats, new_stats)| new_stats - old_stats)
                    .join(future::ok(Operation::Add))
                    .boxify()
            }
        }
    })
    .buffered(100)
    .fold(
        statistics,
        |statistics, (file_stats, operation)| match operation {
            Operation::Add => future::ok::<_, Error>(statistics + file_stats),
            Operation::Sub => future::ok::<_, Error>(statistics - file_stats),
        },
    )
    .map(move |statistics| statistics)
}

pub fn print_statistics(changeset: HgChangesetId, statistics: RepoStatistics) {
    println!(
        "Changeset: {:?}. Number of files: {}, total file size: {}, number of lines: {}",
        changeset, statistics.num_files, statistics.total_file_size, statistics.num_lines
    );
}

pub fn log_statistics(
    mut scuba_logger: ScubaSampleBuilder,
    repo_name: String,
    statistics: RepoStatistics,
) {
    scuba_logger
        .add("repo_name", repo_name)
        .add("num_files", statistics.num_files)
        .add("total_file_size", statistics.total_file_size)
        .add("num_lines", statistics.num_lines)
        .log();
}

enum Pass {
    FirstPass(HgChangesetId),
    NextPass(HgChangesetId, HgChangesetId),
}

enum Operation {
    Add,
    Sub,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches();

    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(&matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let bookmark = match matches.value_of("bookmark") {
        Some(name) => name.to_string(),
        None => String::from("master"),
    };
    let bookmark = BookmarkName::new(bookmark.clone())?;
    let repo_name = args::get_repo_name(&matches)?;
    let scuba_logger = if matches.is_present("log-to-scuba") {
        ScubaSampleBuilder::new(fb, SCUBA_DATASET_NAME)
    } else {
        ScubaSampleBuilder::with_discard()
    };

    let run = args::open_repo(fb, &logger, &matches).and_then(move |repo| {
        let blobstore = Arc::new(repo.get_blobstore());
        repo.get_bookmark(ctx.clone(), &bookmark)
            .and_then(move |changeset| changeset.ok_or(err_msg("cannot load bookmark")))
            .and_then(move |changeset| {
                loop_fn::<_, (), _, _>(
                    (Pass::FirstPass(changeset), RepoStatistics::default()),
                    move |(pass, statistics)| {
                        cloned!(ctx, repo, blobstore, bookmark);
                        match pass {
                            Pass::FirstPass(changeset) => get_statistics_from_changeset(
                                ctx.clone(),
                                repo.clone(),
                                blobstore.clone(),
                                changeset.clone(),
                            )
                            .and_then({
                                cloned!(repo_name, scuba_logger);
                                move |statistics| {
                                    print_statistics(changeset, statistics);
                                    log_statistics(scuba_logger, repo_name, statistics);
                                    future::ok((changeset, statistics))
                                }
                            })
                            .boxify(),
                            Pass::NextPass(prev_changeset, cur_changeset) => {
                                if prev_changeset == cur_changeset {
                                    tokio_timer::sleep(Duration::from_millis(1000))
                                        .from_err()
                                        .map(move |()| (cur_changeset, statistics))
                                        .boxify()
                                } else {
                                    get_manifest_from_changeset(
                                        ctx.clone(),
                                        repo.clone(),
                                        prev_changeset.clone(),
                                    )
                                    .join(get_manifest_from_changeset(
                                        ctx.clone(),
                                        repo.clone(),
                                        cur_changeset.clone(),
                                    ))
                                    .and_then({
                                        cloned!(ctx, repo, repo_name, scuba_logger);
                                        move |(prev_manifest_id, cur_manifest_id)| {
                                            update_statistics(
                                                ctx.clone(),
                                                repo.clone(),
                                                statistics.clone(),
                                                prev_manifest_id.diff(
                                                    ctx.clone(),
                                                    blobstore.clone(),
                                                    cur_manifest_id.clone(),
                                                ),
                                            )
                                            .map(
                                                move |statistics| {
                                                    print_statistics(cur_changeset, statistics);
                                                    log_statistics(
                                                        scuba_logger,
                                                        repo_name,
                                                        statistics,
                                                    );
                                                    (cur_changeset, statistics)
                                                },
                                            )
                                        }
                                    })
                                    .boxify()
                                }
                            }
                        }
                        .and_then(move |(cur_changeset, statistics)| {
                            repo.get_bookmark(ctx.clone(), &bookmark)
                                .and_then(move |new_changeset| {
                                    new_changeset.ok_or(err_msg("cannot load bookmark"))
                                })
                                .and_then(move |new_changeset| {
                                    future::ok(Loop::Continue((
                                        Pass::NextPass(cur_changeset, new_changeset),
                                        statistics,
                                    )))
                                })
                        })
                    },
                )
            })
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run)?;
    runtime.shutdown_on_idle();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use fixtures::linear;
    use futures::stream;
    use maplit::btreemap;
    use std::str::FromStr;
    use tests_utils::{create_commit, store_files};
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

    #[fbinit::test]
    fn linear_test_get_statistics_from_changeset(fb: FacebookInit) {
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let blobstore = repo.get_blobstore();

        // Commit consists two files (name => content):
        //     "1" => "1\n"
        //     "files" => "1\n"
        // */
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let p = repo.get_bonsai_from_hg(ctx.clone(), root);
        let p = runtime.block_on(p).unwrap().unwrap();
        let parents = vec![p];

        let bcs_id = create_commit(
            ctx.clone(),
            repo.clone(),
            parents,
            store_files(
                ctx.clone(),
                btreemap! {
                    "dir1/dir2/file1" => Some("first line\nsecond line\n"),
                    "dir1/dir3/file2" => Some("first line\n"),
                },
                repo.clone(),
            ),
        );

        let hg_cs_id = repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id);
        let hg_cs_id = runtime.block_on(hg_cs_id).unwrap();

        let stats = get_statistics_from_changeset(
            ctx.clone(),
            repo.clone(),
            blobstore.clone(),
            hg_cs_id.clone(),
        );
        let stats = runtime.block_on(stats).unwrap();

        // (num_files, total_file_size, num_lines)
        assert_eq!(stats, RepoStatistics::new(4, 38, 5));
    }

    #[fbinit::test]
    fn linear_test_get_statistics_from_entry_tree(fb: FacebookInit) {
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let blobstore = repo.get_blobstore();

        // Commit consists two files (name => content):
        //     "1" => "1\n"
        //     "files" => "1\n"
        // */
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let p = repo.get_bonsai_from_hg(ctx.clone(), root);
        let p = runtime.block_on(p).unwrap().unwrap();
        let parents = vec![p];

        let bcs_id = create_commit(
            ctx.clone(),
            repo.clone(),
            parents,
            store_files(
                ctx.clone(),
                btreemap! {
                    "dir1/dir2/file1" => Some("first line\nsecond line\n"),
                    "dir1/dir3/file2" => Some("first line\n"),
                },
                repo.clone(),
            ),
        );

        let hg_cs_id = repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id);
        let hg_cs_id = runtime.block_on(hg_cs_id).unwrap();

        let tree_entries = get_manifest_from_changeset(ctx.clone(), repo.clone(), hg_cs_id.clone())
            .and_then({
                cloned!(ctx);
                move |manifest| {
                    manifest
                        .list_all_entries(ctx.clone(), blobstore.clone())
                        .filter_map(|(_, entry)| match entry {
                            Entry::Tree(_) => Some(entry),
                            _ => None,
                        })
                        .collect()
                }
            });
        let mut tree_entries = runtime.block_on(tree_entries).unwrap();

        let stats =
            get_statistics_from_entry(ctx.clone(), repo.clone(), tree_entries.pop().unwrap());
        let stats = runtime.block_on(stats).unwrap();

        // For Entry::Tree we expect repository with all statistics equal 0
        // (num_files, total_file_size, num_lines)
        assert_eq!(stats, RepoStatistics::default());
    }

    #[fbinit::test]
    fn linear_test_update_statistics(fb: FacebookInit) {
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let blobstore = repo.get_blobstore();

        /*
        Commit consists two files (name => content):
            "1" => "1\n"
            "files" => "1\n"
        */
        let prev_hg_cs_id =
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        /*
        Commit consists two files (name => content):
            "2" => "2\n"
            "files" => "1\n2\n"
        */
        let cur_hg_cs_id =
            HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap();

        let stats = get_statistics_from_changeset(
            ctx.clone(),
            repo.clone(),
            blobstore.clone(),
            prev_hg_cs_id.clone(),
        );
        let stats = runtime.block_on(stats).unwrap();

        let manifests =
            get_manifest_from_changeset(ctx.clone(), repo.clone(), prev_hg_cs_id.clone()).join(
                get_manifest_from_changeset(ctx.clone(), repo.clone(), cur_hg_cs_id.clone()),
            );
        let (prev_manifest, cur_manifest) = runtime.block_on(manifests).unwrap();

        let new_stats = update_statistics(
            ctx.clone(),
            repo.clone(),
            stats.clone(),
            prev_manifest.diff(ctx.clone(), blobstore.clone(), cur_manifest.clone()),
        );
        let new_stats = runtime.block_on(new_stats).unwrap();

        // (num_files, total_file_size, num_lines)
        assert_eq!(new_stats, RepoStatistics::new(3, 8, 4));
    }
}

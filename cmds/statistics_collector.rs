// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bookmarks::BookmarkName;
use bytes::Bytes;
use changesets::{deserialize_cs_entries, ChangesetEntry};
use clap::{App, Arg, SubCommand};
use cloned::cloned;
use cmdlib::{args, monitoring};
use context::CoreContext;
use failure::err_msg;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::future;
use futures::future::{loop_fn, Loop};
use futures::future::{Future, IntoFuture};
use futures::stream;
use futures::stream::Stream;
use futures_ext::FutureExt;
use futures_ext::{BoxFuture, BoxStream};
use manifest::{Diff, Entry, ManifestOps};
use mercurial_types::{Changeset, FileBytes, HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::{FileType, RepositoryId};
use scuba_ext::ScubaSampleBuilder;
use slog::info;
use stats::{define_stats, Timeseries};
use std::collections::HashMap;
use std::fs;
use std::ops::{Add, Sub};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

define_stats! {
    prefix = "mononoke.statistics_collector";
    calculated_changesets: timeseries(RATE, SUM),
}

const ARG_IN_FILENAME: &'static str = "in-filename";

const SUBCOMMAND_STATISTICS_FROM_FILE: &'static str = "statistics-from-commits-in-file";

const SCUBA_DATASET_NAME: &str = "mononoke_repository_statistics";
// Tool doesn't count number of lines from files with size greater than 10MB
const BIG_FILE_THRESHOLD: u64 = 10000000;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
    };
    let app = app
        .build("Tool to calculate repo statistic")
        .version("0.0.0")
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_STATISTICS_FROM_FILE)
                .about(
                    "calculate statistics for commits in provided file and save them to json file",
                )
                .arg(
                    Arg::with_name(ARG_IN_FILENAME)
                        .long(ARG_IN_FILENAME)
                        .takes_value(true)
                        .required(true)
                        .help("a file with a list of bonsai changesets to calculate stats for"),
                ),
        )
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
        );
    args::add_fb303_args(app)
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

pub fn get_changeset_timestamp_from_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs_id: HgChangesetId,
) -> impl Future<Item = i64, Error = Error> {
    repo.get_changeset_by_changesetid(ctx.clone(), hg_cs_id.clone())
        .map(move |changeset| changeset.time().timestamp_secs())
}

// Calculates number of lines only for regular-type file
pub fn get_statistics_from_entry(
    ctx: CoreContext,
    repo: BlobRepo,
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> impl Future<Item = RepoStatistics, Error = Error> {
    match entry {
        Entry::Leaf((file_type, filenode_id)) => repo
            .get_file_size(ctx.clone(), filenode_id)
            .and_then(move |size| {
                if FileType::Regular == file_type && size < BIG_FILE_THRESHOLD {
                    number_of_lines(repo.get_file_content(ctx.clone(), filenode_id))
                        .join(future::ok(size))
                        .left_future()
                } else {
                    future::ok((0, size)).right_future()
                }
            })
            .map(move |(lines, size)| RepoStatistics::new(1, size as i64, lines))
            .left_future(),
        Entry::Tree(_) => future::ok(RepoStatistics::default()).right_future(),
    }
}

pub fn get_statistics_from_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    blobstore: impl Blobstore + Clone,
    hg_cs_id: HgChangesetId,
) -> impl Future<Item = RepoStatistics, Error = Error> {
    info!(
        ctx.logger(),
        "Started calculating statistics for changeset {}", hg_cs_id
    );
    get_manifest_from_changeset(ctx.clone(), repo.clone(), hg_cs_id.clone()).and_then({
        cloned!(ctx, repo);
        move |manifest_id| {
            manifest_id
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .map({
                    cloned!(ctx);
                    move |(_, leaf)| {
                        get_statistics_from_entry(ctx.clone(), repo.clone(), Entry::Leaf(leaf))
                    }
                })
                .buffered(100)
                .fold(RepoStatistics::default(), |statistics, new_stat| {
                    future::ok::<_, Error>(statistics + new_stat)
                })
                .map(move |statistics| {
                    info!(
                        ctx.logger(),
                        "Finished calculating statistics for changeset {}", hg_cs_id
                    );
                    statistics
                })
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

pub fn log_statistics(
    ctx: CoreContext,
    mut scuba_logger: ScubaSampleBuilder,
    cs_timestamp: i64,
    repo_name: String,
    hg_cs_id: HgChangesetId,
    statistics: RepoStatistics,
) {
    info!(
        ctx.logger(),
        "Statistics for changeset {}\nCs timestamp: {}\nNumber of files {}\nTotal file size {}\nNumber of lines {}",
        hg_cs_id,
        cs_timestamp,
        statistics.num_files,
        statistics.total_file_size,
        statistics.num_lines
    );
    scuba_logger
        .add("repo_name", repo_name)
        .add("num_files", statistics.num_files)
        .add("total_file_size", statistics.total_file_size)
        .add("num_lines", statistics.num_lines)
        .add("changeset", hg_cs_id.to_hex().to_string())
        .log_with_time(cs_timestamp as u64);
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>, Error> {
    let data = fs::read(file).map_err(Error::from)?;
    deserialize_cs_entries(&Bytes::from(data))
}

pub fn generate_statistics_from_file<P: AsRef<Path>>(
    ctx: CoreContext,
    repo: BlobRepo,
    in_path: P,
) -> BoxFuture<(), Error> {
    // 1 day in seconds
    const REQUIRED_COMMITS_DISTANCE: i64 = 60 * 60 * 24;
    let blobstore = Arc::new(repo.get_blobstore());
    parse_serialized_commits(in_path)
        .into_future()
        .and_then(move |changesets| {
            // Mapping repo-id => (cs_creation_timestamp, hg_cs_id, statistics)
            let repo_stats_map: HashMap<RepositoryId, (i64, HgChangesetId, RepoStatistics)> =
                HashMap::new();
            stream::iter_ok(changesets.clone())
                .map({
                    cloned!(ctx, repo);
                    move |cs_id| {
                        let repo_id = cs_id.repo_id;
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id.cs_id)
                        .and_then({
                            cloned!(ctx, repo);
                            move |hg_cs_id| {
                                get_changeset_timestamp_from_changeset(ctx.clone(), repo.clone(), hg_cs_id)
                                .map(move |cs_timestamp| {
                                    (hg_cs_id, cs_timestamp, repo_id)
                                })
                            }
                        })
                    }
                })
                .buffered(100)
                .collect()
                .map(move |mut changesets| {
                    changesets.sort_by_key(|(_, cs_timestamp, _)| cs_timestamp.clone());
                    stream::iter_ok(changesets)
                })
                .flatten_stream()
                .fold(repo_stats_map, move |mut repo_stats_map, (hg_cs_id, cs_timestamp, repo_id)| {
                    cloned!(ctx, repo, blobstore);
                    if !repo_stats_map.contains_key(&repo_id) {
                        get_statistics_from_changeset(
                            ctx.clone(),
                            repo.clone(),
                            blobstore.clone(),
                            hg_cs_id,
                        )
                        .map(move |statistics| {
                            // TODO save statistics to csv
                            info!(
                                ctx.logger(),
                                "Statistics for repo: {}\nchangeset {}\nCs timestamp: {}\nNumber of files {}\nTotal file size {}\nNumber of lines {}",
                                repo_id.id(),
                                hg_cs_id,
                                cs_timestamp,
                                statistics.num_files,
                                statistics.total_file_size,
                                statistics.num_lines
                            );
                            repo_stats_map.insert(
                                repo_id,
                                (cs_timestamp, hg_cs_id, statistics),
                            );
                            repo_stats_map
                        })
                        .boxify()
                    } else {
                        let (old_cs_timestamp, old_hg_cs_id, old_stats) = repo_stats_map[&repo_id];
                        // Calculate statistics for changeset only if changeset
                        // was created at least REQUIRED_COMMITS_DISTANCE seconds after
                        // changeset we used previously to calculate statistics.
                        if cs_timestamp - old_cs_timestamp > REQUIRED_COMMITS_DISTANCE {
                            get_manifest_from_changeset(
                                ctx.clone(),
                                repo.clone(),
                                old_hg_cs_id.clone(),
                            )
                            .join(get_manifest_from_changeset(
                                ctx.clone(),
                                repo.clone(),
                                hg_cs_id.clone(),
                            ))
                            .and_then(move |(old_manifest, manifest)| {
                                update_statistics(
                                    ctx.clone(),
                                    repo.clone(),
                                    old_stats.clone(),
                                    old_manifest.diff(
                                        ctx.clone(),
                                        blobstore.clone(),
                                        manifest.clone(),
                                    ),
                                )
                                .map(move |statistics| {
                                    // TODO save statistics to csv
                                    info!(
                                        ctx.logger(),
                                        "Statistics for repo: {}\nchangeset {}\nCs timestamp: {}\nNumber of files {}\nTotal file size {}\nNumber of lines {}",
                                        repo_id.id(),
                                        hg_cs_id,
                                        cs_timestamp,
                                        statistics.num_files,
                                        statistics.total_file_size,
                                        statistics.num_lines
                                    );
                                    repo_stats_map.insert(
                                        repo_id,
                                        (cs_timestamp, hg_cs_id, statistics),
                                    );
                                    repo_stats_map
                                })
                            })
                            .boxify()
                        } else {
                            // Skip this changeset
                            future::ok(repo_stats_map).boxify()
                        }
                    }
                })
                .map(move |_| ())
        })
        .boxify()
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

    let run = args::open_repo(fb, &logger, &matches).and_then({
        cloned!(matches);
        move |repo| {
            if let (SUBCOMMAND_STATISTICS_FROM_FILE, Some(sub_m)) = matches.subcommand() {
                cloned!(ctx);
                // Both arguments are set to be required
                let in_filename = sub_m
                    .value_of(ARG_IN_FILENAME)
                    .expect("missing required argument");
                generate_statistics_from_file(ctx.clone(), repo.clone(), in_filename)
            } else {
                let blobstore = Arc::new(repo.get_blobstore());
                repo.get_bookmark(ctx.clone(), &bookmark)
                    .and_then(move |changeset| changeset.ok_or(err_msg("cannot load bookmark")))
                    .and_then(move |changeset| {
                        loop_fn::<_, (), _, _>(
                            (Pass::FirstPass(changeset), RepoStatistics::default()),
                            move |(pass, statistics)| {
                                cloned!(ctx, repo, blobstore, bookmark);
                                match pass {
                                    Pass::FirstPass(changeset) => {
                                        get_statistics_from_changeset(
                                            ctx.clone(),
                                            repo.clone(),
                                            blobstore.clone(),
                                            changeset.clone(),
                                        )
                                        .and_then({
                                            cloned!(repo, repo_name, scuba_logger, ctx);
                                            move |statistics| {
                                                get_changeset_timestamp_from_changeset(
                                                    ctx.clone(),
                                                    repo,
                                                    changeset,
                                                )
                                                .map(move |cs_timestamp| {
                                                    log_statistics(
                                                        ctx,
                                                        scuba_logger,
                                                        cs_timestamp,
                                                        repo_name,
                                                        changeset,
                                                        statistics,
                                                    );
                                                    STATS::calculated_changesets.add_value(1);
                                                    (changeset, statistics)
                                                })
                                            }
                                        })
                                        .boxify()
                                    }
                                    Pass::NextPass(prev_changeset, cur_changeset) => {
                                        if prev_changeset == cur_changeset {
                                            let duration = Duration::from_millis(1000);
                                            info!(
                                                ctx.logger(),
                                                "Changeset hasn't changed, sleeping {:?}", duration
                                            );
                                            tokio_timer::sleep(duration)
                                                .from_err()
                                                .map(move |()| (cur_changeset, statistics))
                                                .boxify()
                                        } else {
                                            info!(
                                                ctx.logger(),
                                                "Found new changeset: {}, updating statistics",
                                                cur_changeset
                                            );
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
                                                    .and_then({
                                                        cloned!(ctx);
                                                        info!(
                                                            ctx.logger(),
                                                            "Statistics for new changeset updated."
                                                        );
                                                        move |statistics| {
                                                            get_changeset_timestamp_from_changeset(
                                                                ctx.clone(),
                                                                repo,
                                                                cur_changeset,
                                                            )
                                                            .map(move |cs_timestamp| {
                                                                log_statistics(
                                                                    ctx,
                                                                    scuba_logger,
                                                                    cs_timestamp,
                                                                    repo_name,
                                                                    cur_changeset,
                                                                    statistics,
                                                                );
                                                                STATS::calculated_changesets
                                                                    .add_value(1);
                                                                (cur_changeset, statistics)
                                                            })
                                                        }
                                                    })
                                                }
                                            })
                                            .boxify()
                                        }
                                    }
                                }
                                .and_then(
                                    move |(cur_changeset, statistics)| {
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
                                    },
                                )
                            },
                        )
                    })
                    .boxify()
            }
        }
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    monitoring::start_fb303_and_stats_agg(
        fb,
        &mut runtime,
        "statistics_collector",
        &logger,
        &matches,
    )?;

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

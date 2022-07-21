/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Blobstore;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bytes::Bytes;
use changesets::deserialize_cs_entries;
use changesets::ChangesetEntry;
use clap_old::Arg;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures_ext::BoxStream;
use futures_ext::StreamExt as OldStreamExt;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::FileBytes;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::FileType;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::info;
use slog::Logger;
use stats::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::ops::Add;
use std::ops::Sub;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

define_stats! {
    prefix = "mononoke.statistics_collector";
    calculated_changesets: timeseries(Rate, Sum),
}

const ARG_IN_FILENAME: &str = "in-filename";

const SUBCOMMAND_STATISTICS_FROM_FILE: &str = "statistics-from-commits-in-file";

const SCUBA_DATASET_NAME: &str = "mononoke_repository_statistics";
// Tool doesn't count number of lines from files with size greater than 10MB
const BIG_FILE_THRESHOLD: u64 = 10000000;

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Tool to calculate repo statistic")
        .with_fb303_args()
        .build()
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

pub async fn number_of_lines(
    bytes_stream: impl Stream<Item = Result<FileBytes, Error>>,
) -> Result<i64, Error> {
    bytes_stream
        .map_ok(|bytes| {
            bytes.into_iter().fold(0, |num_lines, byte| {
                if byte == b'\n' {
                    num_lines + 1
                } else {
                    num_lines
                }
            })
        })
        .try_fold(0, |result, num_lines| async move {
            Ok::<_, Error>(result + num_lines)
        })
        .await
}

pub async fn get_manifest_from_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hg_cs_id: &HgChangesetId,
) -> Result<HgManifestId, Error> {
    let changeset = hg_cs_id.load(ctx, repo.blobstore()).await?;
    Ok(changeset.manifestid())
}

pub async fn get_changeset_timestamp_from_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hg_cs_id: &HgChangesetId,
) -> Result<i64, Error> {
    let changeset = hg_cs_id.load(ctx, repo.blobstore()).await?;
    Ok(changeset.time().timestamp_secs())
}

// Calculates number of lines only for regular-type file
pub async fn get_statistics_from_entry(
    ctx: &CoreContext,
    repo: &BlobRepo,
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> Result<RepoStatistics, Error> {
    match entry {
        Entry::Leaf((file_type, filenode_id)) => {
            let envelope = filenode_id.load(ctx, repo.blobstore()).await?;
            let size = envelope.content_size();
            let content_id = envelope.content_id();
            let lines = if FileType::Regular == file_type && size < BIG_FILE_THRESHOLD {
                let content = filestore::fetch_stream(repo.blobstore(), ctx.clone(), content_id)
                    .map_ok(FileBytes);
                number_of_lines(content).await?
            } else {
                0
            };
            Ok(RepoStatistics::new(1, size as i64, lines))
        }
        Entry::Tree(_) => Ok(RepoStatistics::default()),
    }
}

pub async fn get_statistics_from_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    blobstore: &(impl Blobstore + Clone + 'static),
    hg_cs_id: &HgChangesetId,
) -> Result<RepoStatistics, Error> {
    info!(
        ctx.logger(),
        "Started calculating statistics for changeset {}", hg_cs_id
    );

    let manifest_id = get_manifest_from_changeset(ctx, repo, hg_cs_id).await?;
    let statistics = manifest_id
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map(move |result| match result {
            Ok((_, leaf)) => get_statistics_from_entry(ctx, repo, Entry::Leaf(leaf)).boxed(),
            Err(e) => async move { Err(e) }.boxed(),
        })
        .buffered(100usize)
        .try_fold(
            RepoStatistics::default(),
            |statistics, new_stat| async move { Ok::<_, Error>(statistics + new_stat) },
        )
        .await?;

    info!(
        ctx.logger(),
        "Finished calculating statistics for changeset {}", hg_cs_id
    );

    Ok(statistics)
}

pub async fn update_statistics<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    statistics: RepoStatistics,
    diff: BoxStream<Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>, Error>,
) -> Result<RepoStatistics, Error> {
    diff.compat()
        .map(move |result| async move {
            let diff = result?;
            match diff {
                Diff::Added(_, entry) => {
                    let stat = get_statistics_from_entry(ctx, repo, entry).await?;
                    Ok((stat, Operation::Add))
                }
                Diff::Removed(_, entry) => {
                    let stat = get_statistics_from_entry(ctx, repo, entry).await?;
                    Ok((stat, Operation::Sub))
                }
                Diff::Changed(_, old_entry, new_entry) => {
                    let (old_stats, new_stats) = try_join!(
                        get_statistics_from_entry(ctx, repo, old_entry),
                        get_statistics_from_entry(ctx, repo, new_entry),
                    )?;
                    let stat = new_stats - old_stats;
                    Ok((stat, Operation::Add))
                }
            }
        })
        .buffered(100usize)
        .try_fold(
            statistics,
            |statistics, (file_stats, operation)| async move {
                match operation {
                    Operation::Add => Ok::<_, Error>(statistics + file_stats),
                    Operation::Sub => Ok::<_, Error>(statistics - file_stats),
                }
            },
        )
        .await
}

pub fn log_statistics(
    ctx: &CoreContext,
    mut scuba_logger: MononokeScubaSampleBuilder,
    cs_timestamp: i64,
    repo_name: &str,
    hg_cs_id: &HgChangesetId,
    statistics: &RepoStatistics,
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
        .add("repo_name", repo_name.to_owned())
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

pub async fn generate_statistics_from_file<P: AsRef<Path>>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    in_path: &P,
) -> Result<(), Error> {
    // 1 day in seconds
    const REQUIRED_COMMITS_DISTANCE: i64 = 60 * 60 * 24;
    let blobstore = Arc::new(repo.get_blobstore());
    // TODO(dgrzegorzewski): T55705023 consider creating csv file here and save statistics using
    // e.g. serde deserialize. To avoid saving fields separately it may be necessary to add new
    // fields to RepoStatistics struct, like cs_timestamp, hg_cs_id, repo_id and refactor code.
    println!("repo_id,hg_cs_id,cs_timestamp,num_files,total_file_size,num_lines");
    let changesets = parse_serialized_commits(in_path)?;
    info!(ctx.logger(), "Started calculating changesets timestamps");

    let mut changeset_info_vec = stream::iter(changesets)
        .map({
            move |changeset| async move {
                let ChangesetEntry { repo_id, cs_id, .. } = changeset;
                let hg_cs_id = repo.derive_hg_changeset(ctx, cs_id).await?;
                let cs_timestamp =
                    get_changeset_timestamp_from_changeset(ctx, repo, &hg_cs_id).await?;
                // the error type annotation in principle should be inferred,
                // but the compiler currently needs it. See https://fburl.com/n1s2ujjb
                Ok::<(HgChangesetId, i64, RepositoryId), Error>((hg_cs_id, cs_timestamp, repo_id))
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    info!(
        ctx.logger(),
        "Timestamps calculated, sorting them and starting calculating statistics"
    );
    changeset_info_vec.sort_by_key(|(_, cs_timestamp, _)| cs_timestamp.clone());

    // accumulate stats into a map
    let mut repo_stats_map = HashMap::<RepositoryId, (i64, HgChangesetId, RepoStatistics)>::new();
    for (hg_cs_id, cs_timestamp, repo_id) in changeset_info_vec {
        match repo_stats_map.get(&repo_id).cloned() {
            Some((old_cs_timestamp, old_hg_cs_id, old_stats)) => {
                // Calculate statistics for changeset only if changeset
                // was created at least REQUIRED_COMMITS_DISTANCE seconds after
                // changeset we used previously to calculate statistics.
                if cs_timestamp - old_cs_timestamp <= REQUIRED_COMMITS_DISTANCE {
                    continue;
                }
                info!(
                    ctx.logger(),
                    "Changeset {} with timestamp {} was created more than {} seconds after previous, calculating statistics for it",
                    hg_cs_id,
                    cs_timestamp,
                    REQUIRED_COMMITS_DISTANCE
                );
                let (old_manifest, manifest) = try_join!(
                    get_manifest_from_changeset(ctx, repo, &old_hg_cs_id,),
                    get_manifest_from_changeset(ctx, repo, &hg_cs_id),
                )?;
                let statistics = update_statistics(
                    ctx,
                    repo,
                    old_stats,
                    old_manifest
                        .diff(ctx.clone(), blobstore.clone(), manifest.clone())
                        .compat()
                        .boxify(),
                )
                .await?;

                info!(
                    ctx.logger(),
                    "Statistics for changeset {} calculated", hg_cs_id
                );
                println!(
                    "{},{},{},{},{},{}",
                    repo_id.id(),
                    hg_cs_id.to_hex(),
                    cs_timestamp,
                    statistics.num_files,
                    statistics.total_file_size,
                    statistics.num_lines
                );
                repo_stats_map.insert(repo_id, (cs_timestamp, hg_cs_id, statistics));
            }
            None => {
                info!(
                    ctx.logger(),
                    "Found first changeset for repo_id {}",
                    repo_id.id()
                );
                let statistics =
                    get_statistics_from_changeset(ctx, repo, &blobstore, &hg_cs_id).await?;

                info!(
                    ctx.logger(),
                    "First changeset for repo_id {} calculated",
                    repo_id.id()
                );
                println!(
                    "{},{},{},{},{},{}",
                    repo_id.id(),
                    hg_cs_id.to_hex(),
                    cs_timestamp,
                    statistics.num_files,
                    statistics.total_file_size,
                    statistics.num_lines
                );
                repo_stats_map.insert(repo_id, (cs_timestamp, hg_cs_id, statistics));
            }
        }
    }
    Ok(())
}

enum Operation {
    Add,
    Sub,
}

#[allow(unreachable_code)]
async fn run_statistics<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    logger: &Logger,
    scuba_logger: MononokeScubaSampleBuilder,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
    bookmark: BookmarkName,
) -> Result<(), Error> {
    let repo = args::open_repo(fb, logger, matches).await?;

    if let (SUBCOMMAND_STATISTICS_FROM_FILE, Some(sub_m)) = matches.subcommand() {
        // Both arguments are set to be required
        let in_filename = sub_m
            .value_of(ARG_IN_FILENAME)
            .expect("missing required argument");
        return generate_statistics_from_file(&ctx, &repo, &in_filename).await;
    }

    let blobstore = Arc::new(repo.get_blobstore());
    let mut changeset = repo
        .get_bookmark(ctx.clone(), &bookmark)
        .await?
        .ok_or_else(|| Error::msg("cannot load bookmark"))?;

    // initialize the loop

    let mut statistics = get_statistics_from_changeset(&ctx, &repo, &blobstore, &changeset).await?;

    let cs_timestamp = get_changeset_timestamp_from_changeset(&ctx, &repo, &changeset).await?;

    log_statistics(
        &ctx,
        scuba_logger.clone(),
        cs_timestamp,
        &repo_name,
        &changeset,
        &statistics,
    );

    STATS::calculated_changesets.add_value(1);

    // run the loop
    loop {
        let prev_changeset = changeset;
        changeset = repo
            .get_bookmark(ctx.clone(), &bookmark)
            .await?
            .ok_or_else(|| Error::msg("cannot load bookmark"))?;

        if prev_changeset == changeset {
            let duration = Duration::from_millis(1000);
            info!(
                ctx.logger(),
                "Changeset hasn't changed, sleeping {:?}", duration
            );

            sleep(duration).await;
        } else {
            info!(
                ctx.logger(),
                "Found new changeset: {}, updating statistics", changeset
            );

            let (prev_manifest_id, cur_manifest_id) = try_join!(
                get_manifest_from_changeset(&ctx, &repo, &prev_changeset),
                get_manifest_from_changeset(&ctx, &repo, &changeset),
            )?;

            statistics = update_statistics(
                &ctx,
                &repo,
                statistics,
                prev_manifest_id
                    .diff(ctx.clone(), blobstore.clone(), cur_manifest_id.clone())
                    .compat()
                    .boxify(),
            )
            .await?;

            info!(ctx.logger(), "Statistics for new changeset updated.");

            let cs_timestamp =
                get_changeset_timestamp_from_changeset(&ctx, &repo, &changeset).await?;

            log_statistics(
                &ctx,
                scuba_logger.clone(),
                cs_timestamp,
                &repo_name,
                &changeset,
                &statistics,
            );
            STATS::calculated_changesets.add_value(1);
        }
    }

    // unreachable, but needed so that the future has type Result
    // which lets us propagate Errors to main.
    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let config_store = matches.config_store();
    let bookmark = match matches.value_of("bookmark") {
        Some(name) => name.to_string(),
        None => String::from("master"),
    };
    let bookmark = BookmarkName::new(bookmark)?;
    let repo_name = args::get_repo_name(config_store, &matches)?;
    let scuba_logger = if matches.is_present("log-to-scuba") {
        MononokeScubaSampleBuilder::new(fb, SCUBA_DATASET_NAME)
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };

    block_execute(
        run_statistics(fb, ctx, logger, scuba_logger, &matches, repo_name, bookmark),
        fb,
        "statistics_collector",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrowed::borrowed;
    use bytes::Bytes;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::future;
    use futures::stream;
    use maplit::btreemap;
    use std::str::FromStr;
    use tests_utils::create_commit;
    use tests_utils::store_files;
    use tokio::runtime::Runtime;

    #[test]
    fn test_number_of_lines_empty_stream() -> Result<(), Error> {
        let rt = Runtime::new().unwrap();

        let stream = stream::once(async { Ok(FileBytes(Bytes::from(&b""[..]))) });
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 0);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_one_line() -> Result<(), Error> {
        let rt = Runtime::new().unwrap();

        let stream = stream::once(async { Ok(FileBytes(Bytes::from(&b"First line\n"[..]))) });
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 1);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_many_lines() -> Result<(), Error> {
        let rt = Runtime::new().unwrap();

        let stream = stream::once(async {
            Ok(FileBytes(Bytes::from(
                &b"First line\nSecond line\nThird line\n"[..],
            )))
        });
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 3);
        Ok(())
    }

    #[test]
    fn test_number_of_lines_many_items() -> Result<(), Error> {
        let rt = Runtime::new().unwrap();

        let vec = vec![
            FileBytes(Bytes::from(&b"First line\n"[..])),
            FileBytes(Bytes::from(&b""[..])),
            FileBytes(Bytes::from(&b"First line\nSecond line\nThird line\n"[..])),
        ];
        let stream = stream::iter(vec.into_iter().map(Ok));
        let result = rt.block_on(number_of_lines(stream))?;
        assert_eq!(result, 4);
        Ok(())
    }

    #[fbinit::test]
    fn linear_test_get_statistics_from_changeset(fb: FacebookInit) {
        let runtime = Runtime::new().unwrap();
        runtime.block_on(async move {
            let repo = Linear::getrepo(fb).await;

            let ctx = CoreContext::test_mock(fb);
            let blobstore = repo.get_blobstore();
            borrowed!(ctx, blobstore, repo);

            // Commit consists two files (name => content):
            //     "1" => "1\n"
            //     "files" => "1\n"
            // */
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(ctx, root)
                .await
                .unwrap()
                .unwrap();
            let parents = vec![p];

            let bcs_id = create_commit(
                ctx.clone(),
                repo.clone(),
                parents,
                store_files(
                    ctx,
                    btreemap! {
                        "dir1/dir2/file1" => Some("first line\nsecond line\n"),
                        "dir1/dir3/file2" => Some("first line\n"),
                    },
                    repo,
                )
                .await,
            )
            .await;

            let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await.unwrap();

            let stats = get_statistics_from_changeset(ctx, repo, blobstore, &hg_cs_id)
                .await
                .unwrap();

            // (num_files, total_file_size, num_lines)
            assert_eq!(stats, RepoStatistics::new(4, 38, 5));
        });
    }

    #[fbinit::test]
    fn linear_test_get_statistics_from_entry_tree(fb: FacebookInit) {
        let runtime = Runtime::new().unwrap();
        runtime.block_on(async move {
            let repo = Linear::getrepo(fb).await;

            let ctx = CoreContext::test_mock(fb);
            let blobstore = repo.get_blobstore();
            borrowed!(ctx, blobstore, repo);

            // Commit consists two files (name => content):
            //     "1" => "1\n"
            //     "files" => "1\n"
            // */
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(ctx, root)
                .await
                .unwrap()
                .unwrap();
            let parents = vec![p];

            let bcs_id = create_commit(
                ctx.clone(),
                repo.clone(),
                parents,
                store_files(
                    ctx,
                    btreemap! {
                        "dir1/dir2/file1" => Some("first line\nsecond line\n"),
                        "dir1/dir3/file2" => Some("first line\n"),
                    },
                    repo,
                )
                .await,
            )
            .await;

            let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await.unwrap();

            let manifest = get_manifest_from_changeset(ctx, repo, &hg_cs_id)
                .await
                .unwrap();

            let mut tree_entries = manifest
                .list_all_entries(ctx.clone(), blobstore.clone())
                .try_filter_map(|(_, entry)| match entry {
                    Entry::Tree(_) => future::ok(Some(entry)),
                    _ => future::ok(None),
                })
                .try_collect::<Vec<_>>()
                .await
                .unwrap();

            let stats = get_statistics_from_entry(ctx, repo, tree_entries.pop().unwrap())
                .await
                .unwrap();

            // For Entry::Tree we expect repository with all statistics equal 0
            // (num_files, total_file_size, num_lines)
            assert_eq!(stats, RepoStatistics::default());
        });
    }

    #[fbinit::test]
    fn linear_test_update_statistics(fb: FacebookInit) {
        let runtime = Runtime::new().unwrap();
        runtime.block_on(async move {
            let repo = Linear::getrepo(fb).await;

            let ctx = CoreContext::test_mock(fb);
            let blobstore = repo.get_blobstore();
            borrowed!(ctx, blobstore, repo);

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

            let stats = get_statistics_from_changeset(ctx, repo, blobstore, &prev_hg_cs_id)
                .await
                .unwrap();

            let (prev_manifest, cur_manifest) = try_join!(
                get_manifest_from_changeset(ctx, repo, &prev_hg_cs_id),
                get_manifest_from_changeset(ctx, repo, &cur_hg_cs_id),
            )
            .unwrap();

            let new_stats = update_statistics(
                ctx,
                repo,
                stats,
                prev_manifest
                    .diff(ctx.clone(), blobstore.clone(), cur_manifest.clone())
                    .compat()
                    .boxify(),
            )
            .await
            .unwrap();

            // (num_files, total_file_size, num_lines)
            assert_eq!(new_stats, RepoStatistics::new(3, 8, 4));
        });
    }
}

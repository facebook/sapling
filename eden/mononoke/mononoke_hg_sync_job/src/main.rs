/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(optin_builtin_traits)]
#![feature(negative_impls)]
#![deny(warnings)]

/// Mononoke -> hg sync job
///
/// It's a special job that is used to synchronize Mononoke to Mercurial when Mononoke is a source
/// of truth. All writes to Mononoke are replayed to Mercurial using this job. That can be used
/// to verify Mononoke's correctness and/or use hg as a disaster recovery mechanism.
use anyhow::{bail, format_err, Error, Result};
use blobrepo_hg::BlobRepoHg;
use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateLogEntry, Freshness};
use bundle_generator::FilenodeVerifier;
use bundle_preparer::{BundlePreparer, PreparedBookmarkUpdateLogEntry};
use clap::{Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers::block_execute};
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, FutureExt as _, TryFutureExt},
    stream::TryStreamExt,
};
use futures_ext::{spawn_future, try_boxfuture, BoxFuture, FutureExt, StreamExt};
use futures_old::{
    future::{err, join_all, ok, IntoFuture},
    stream,
    stream::Stream,
    Future,
};
use futures_stats::{FutureStats, Timed};
use http::Uri;
use lfs_verifier::LfsVerifier;
use mercurial_types::HgChangesetId;
use metaconfig_types::HgsqlName;
use metaconfig_types::RepoReadOnly;
use mononoke_hg_sync_job_helper_lib::{
    merge_bundles, merge_timestamp_files, retry, RetryAttemptsCount,
};
use mononoke_types::{ChangesetId, RepositoryId};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use regex::Regex;
use repo_read_write_status::{RepoReadWriteFetcher, SqlRepoReadWriteStatus};
use scuba_ext::ScubaSampleBuilder;
use slog::{error, info};
use sql_construct::{facebook::FbSqlConstruct, SqlConstruct};
use sql_ext::facebook::{myrouter_ready, MysqlOptions};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio_timer::sleep;

mod bundle_generator;
mod bundle_preparer;
mod errors;
mod globalrev_syncer;
mod hgrepo;
mod lfs_verifier;

use errors::{
    ErrorKind::SyncFailed,
    PipelineError::{self, AnonymousError, EntryError},
};
use globalrev_syncer::GlobalrevSyncer;
use hgrepo::{list_hg_server_bookmarks, HgRepo};
use hgserver_config::ServerConfig;

const ARG_BOOKMARK_REGEX_FORCE_GENERATE_LFS: &str = "bookmark-regex-force-generate-lfs";
const GENERATE_BUNDLES: &str = "generate-bundles";
const MODE_SYNC_ONCE: &'static str = "sync-once";
const MODE_SYNC_LOOP: &'static str = "sync-loop";
const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";
const SLEEP_SECS: u64 = 1;
const SCUBA_TABLE: &'static str = "mononoke_hg_sync";
const UNLOCK_REASON: &str = "Unlocked by successful sync";
const LOCK_REASON: &str = "Locked due to sync failure, check Source Control @ FB";

const HGSQL_GLOBALREVS_USE_SQLITE: &str = "hgsql-globalrevs-use-sqlite";
const HGSQL_GLOBALREVS_DB_ADDR: &str = "hgsql-globalrevs-db-addr";

const DEFAULT_RETRY_NUM: usize = 3;
const DEFAULT_BATCH_SIZE: usize = 10;
const DEFAULT_SINGLE_BUNDLE_TIMEOUT_MS: u64 = 5 * 60 * 1000;

const CONFIGERATOR_HGSERVER_PATH: &str = "configerator:scm/mononoke/hgserverconf/hgserver";

#[derive(Copy, Clone)]
struct QueueSize(usize);

struct PipelineState<T> {
    entries: Vec<BookmarkUpdateLogEntry>,
    data: T,
}

type OutcomeWithStats =
    Result<(FutureStats, PipelineState<RetryAttemptsCount>), (Option<FutureStats>, PipelineError)>;

type Outcome = Result<PipelineState<RetryAttemptsCount>, PipelineError>;

fn get_id_to_search_after(entries: &[BookmarkUpdateLogEntry]) -> i64 {
    entries.iter().map(|entry| entry.id).max().unwrap_or(0)
}

fn bind_sync_err(entries: &[BookmarkUpdateLogEntry], cause: Error) -> PipelineError {
    let ids: Vec<i64> = entries.iter().map(|entry| entry.id).collect();
    let entries = entries.to_vec();
    EntryError {
        entries,
        cause: (SyncFailed { ids, cause }).into(),
    }
}

fn bind_sync_result<T>(
    entries: &[BookmarkUpdateLogEntry],
    res: Result<T>,
) -> Result<PipelineState<T>, PipelineError> {
    match res {
        Ok(data) => Ok(PipelineState {
            entries: entries.to_vec(),
            data,
        }),
        Err(cause) => Err(bind_sync_err(entries, cause)),
    }
}

fn drop_outcome_stats(o: OutcomeWithStats) -> Outcome {
    o.map(|(_, r)| r).map_err(|(_, e)| e)
}

fn build_reporting_handler(
    ctx: CoreContext,
    scuba_sample: ScubaSampleBuilder,
    retry_num: usize,
    bookmarks: impl BookmarkUpdateLog,
) -> impl Fn(OutcomeWithStats) -> BoxFuture<PipelineState<RetryAttemptsCount>, PipelineError> {
    move |res| {
        cloned!(ctx, scuba_sample);

        let log_entries = match &res {
            Ok((_, pipeline_state, ..)) => Some(pipeline_state.entries.clone()),
            Err((_, EntryError { entries, .. })) => Some(entries.clone()),
            Err((_, AnonymousError { .. })) => None,
        };

        let maybe_stats = match &res {
            Ok((stats, _)) => Some(stats),
            Err((stats, _)) => stats.as_ref(),
        };

        // TODO: (torozco) T43766262 We should embed attempts in retry()'s Error type and use it
        // here instead of receiving a plain ErrorKind and implicitly assuming retry_num attempts.
        let attempts = match &res {
            Ok((_, PipelineState { data: attempts, .. })) => attempts.clone(),
            Err(..) => RetryAttemptsCount(retry_num),
        };

        let maybe_error = match &res {
            Ok(..) => None,
            Err((_, EntryError { cause, .. })) => Some(cause),
            Err((_, AnonymousError { cause, .. })) => Some(cause),
        };

        let fut = match log_entries {
            None => ok(()).right_future(),
            Some(log_entries) => {
                if log_entries.len() == 0 {
                    err(Error::msg("unexpected empty pipeline state")).right_future()
                } else {
                    let duration = maybe_stats
                        .map(|s| s.completion_time)
                        .unwrap_or(Duration::from_secs(0));

                    let error = maybe_error.map(|e| format!("{:?}", e));
                    let next_id = get_id_to_search_after(&log_entries);

                    bookmarks
                        .count_further_bookmark_log_entries(ctx.clone(), next_id as u64, None)
                        .compat()
                        .map(|n| QueueSize(n as usize))
                        .map({
                            cloned!(log_entries);
                            move |queue_size| {
                                info!(
                                    ctx.logger(),
                                    "queue size after processing: {}", queue_size.0
                                );
                                log_processed_entries_to_scuba(
                                    &log_entries,
                                    scuba_sample,
                                    error,
                                    attempts,
                                    duration,
                                    queue_size,
                                );
                            }
                        })
                        .left_future()
                }
            }
        };

        fut.then(|_| drop_outcome_stats(res)).boxify()
    }
}

fn get_read_write_fetcher(
    mysql_options: MysqlOptions,
    repo_lock_db_addr: Option<&str>,
    hgsql_name: HgsqlName,
    lock_on_failure: bool,
    use_sqlite: bool,
    readonly_storage: bool,
) -> Result<(Option<RepoReadWriteFetcher>, RepoReadWriteFetcher)> {
    let unlock_via: Result<RepoReadWriteFetcher> = match repo_lock_db_addr {
        Some(repo_lock_db_addr) => {
            let sql_repo_read_write_status = if use_sqlite {
                let path = Path::new(repo_lock_db_addr);
                SqlRepoReadWriteStatus::with_sqlite_path(path, readonly_storage)
            } else {
                match mysql_options.myrouter_port {
                    Some(myrouter_port) => Ok(SqlRepoReadWriteStatus::with_myrouter(
                        repo_lock_db_addr.to_string(),
                        myrouter_port,
                        mysql_options.read_connection_type(),
                        readonly_storage,
                    )),
                    None => Err(Error::msg("myrouter_port not specified in mysql mode")),
                }
            };
            sql_repo_read_write_status.and_then(|connection| {
                Ok(RepoReadWriteFetcher::new(
                    Some(connection),
                    RepoReadOnly::ReadWrite,
                    hgsql_name,
                ))
            })
        }
        None => {
            if lock_on_failure {
                Err(Error::msg(
                    "repo_lock_db_addr not specified with lock_on_failure",
                ))
            } else {
                Ok(RepoReadWriteFetcher::new(
                    None,
                    RepoReadOnly::ReadWrite,
                    hgsql_name,
                ))
            }
        }
    };

    unlock_via.and_then(|v| {
        let lock_via = if lock_on_failure {
            Some(v.clone())
        } else {
            None
        };
        Ok((lock_via, v))
    })
}

fn unlock_repo_if_locked(
    ctx: CoreContext,
    read_write_fetcher: RepoReadWriteFetcher,
) -> impl Future<Item = (), Error = Error> {
    read_write_fetcher
        .readonly()
        .and_then(move |repo_state| match repo_state {
            RepoReadOnly::ReadOnly(ref lock_msg) if lock_msg == LOCK_REASON => read_write_fetcher
                .set_mononoke_read_write(&UNLOCK_REASON.to_string())
                .map(move |updated| {
                    if updated {
                        info!(ctx.logger(), "repo is unlocked");
                    }
                })
                .left_future(),
            RepoReadOnly::ReadOnly(..) | RepoReadOnly::ReadWrite => ok(()).right_future(),
        })
}

fn lock_repo_if_unlocked(
    ctx: CoreContext,
    read_write_fetcher: RepoReadWriteFetcher,
) -> impl Future<Item = (), Error = Error> {
    info!(ctx.logger(), "locking repo...");
    read_write_fetcher
        .readonly()
        .and_then(move |repo_state| match repo_state {
            RepoReadOnly::ReadWrite => read_write_fetcher
                .set_read_only(&LOCK_REASON.to_string())
                .map(move |updated| {
                    if updated {
                        info!(ctx.logger(), "repo is locked now");
                    }
                })
                .left_future(),

            RepoReadOnly::ReadOnly(ref lock_msg) => {
                ok(info!(ctx.logger(), "repo is locked already: {}", lock_msg)).right_future()
            }
        })
}

fn build_outcome_handler(
    ctx: CoreContext,
    lock_via: Option<RepoReadWriteFetcher>,
) -> impl Fn(Outcome) -> BoxFuture<Vec<BookmarkUpdateLogEntry>, Error> {
    move |res| match res {
        Ok(PipelineState { entries, .. }) => {
            info!(
                ctx.logger(),
                "successful sync of entries {:?}",
                entries.iter().map(|c| c.id).collect::<Vec<_>>()
            );
            ok(entries).boxify()
        }
        Err(AnonymousError { cause: e }) => {
            info!(ctx.logger(), "error without entry");
            err(e.into()).boxify()
        }
        Err(EntryError { cause: e, .. }) => match &lock_via {
            Some(repo_read_write_fetcher) => {
                cloned!(ctx, repo_read_write_fetcher);
                lock_repo_if_unlocked(ctx, repo_read_write_fetcher)
                    .then(move |_| err(e.into()))
                    .boxify()
            }
            None => err(e.into()).boxify(),
        },
    }
}

#[derive(Clone)]
struct CombinedBookmarkUpdateLogEntry {
    components: Vec<BookmarkUpdateLogEntry>,
    bundle_file: Arc<NamedTempFile>,
    timestamps_file: Arc<NamedTempFile>,
    cs_id: Option<(ChangesetId, HgChangesetId)>,
    bookmark: BookmarkName,
}

fn combine_entries(
    ctx: CoreContext,
    entries: &[PreparedBookmarkUpdateLogEntry],
) -> impl Future<Item = CombinedBookmarkUpdateLogEntry, Error = Error> {
    let bundle_file_paths: Vec<PathBuf> = entries
        .iter()
        .map(|prepared_entry| prepared_entry.bundle_file.path().to_path_buf())
        .collect();
    let timestamp_file_paths: Vec<PathBuf> = entries
        .iter()
        .map(|prepared_entry| prepared_entry.timestamps_file.path().to_path_buf())
        .collect();
    let components: Vec<_> = entries
        .iter()
        .map(|prepared_entry| prepared_entry.log_entry.clone())
        .collect();
    let last_entry = match entries.iter().last() {
        None => {
            return err(Error::msg(
                "cannot create a combined entry from an empty list",
            ))
            .left_future()
        }
        Some(entry) => entry.clone(),
    };

    async move {
        try_join(
            merge_bundles(&ctx, &bundle_file_paths),
            merge_timestamp_files(&ctx, &timestamp_file_paths),
        )
        .await
    }
    .boxed()
    .compat()
    .map(move |(combined_bundle_file, combined_timestamps_file)| {
        let PreparedBookmarkUpdateLogEntry {
            cs_id, log_entry, ..
        } = last_entry;
        CombinedBookmarkUpdateLogEntry {
            components,
            bundle_file: Arc::new(combined_bundle_file),
            timestamps_file: Arc::new(combined_timestamps_file),
            cs_id,
            bookmark: log_entry.bookmark_name,
        }
    })
    .right_future()
}

/// Sends a downloaded bundle to hg
fn try_sync_single_combined_entry(
    ctx: CoreContext,
    attempt: usize,
    combined_entry: CombinedBookmarkUpdateLogEntry,
    hg_repo: HgRepo,
) -> impl Future<Item = (), Error = Error> {
    let CombinedBookmarkUpdateLogEntry {
        components,
        bundle_file,
        timestamps_file,
        cs_id,
        bookmark,
    } = combined_entry;
    let ids: Vec<_> = components.iter().map(|entry| entry.id).collect();
    info!(ctx.logger(), "syncing log entries {:?} ...", ids);

    let bundle_path = try_boxfuture!(get_path(&bundle_file));
    let timestamps_path = try_boxfuture!(get_path(&timestamps_file));

    hg_repo
        .apply_bundle(
            bundle_path,
            timestamps_path,
            bookmark,
            cs_id.map(|(_bcs_id, hg_cs_id)| hg_cs_id),
            attempt,
            ctx.logger().clone(),
        )
        .map(move |()| {
            // Make sure temp file is dropped only after bundle was applied is done
            let _ = bundle_file;
            let _ = timestamps_file;
        })
        .boxify()
}

fn sync_single_combined_entry(
    ctx: CoreContext,
    combined_entry: CombinedBookmarkUpdateLogEntry,
    hg_repo: HgRepo,
    base_retry_delay_ms: u64,
    retry_num: usize,
    globalrev_syncer: GlobalrevSyncer,
) -> impl Future<Item = RetryAttemptsCount, Error = Error> {
    let sync_globalrevs = if let Some((cs_id, _hg_cs_id)) = combined_entry.cs_id {
        async move { globalrev_syncer.sync(cs_id).await }
            .boxed()
            .compat()
            .left_future()
    } else {
        Ok(()).into_future().right_future()
    };

    sync_globalrevs.and_then(move |()| {
        retry(
            ctx.logger().clone(),
            {
                cloned!(ctx, combined_entry);
                move |attempt| {
                    try_sync_single_combined_entry(
                        ctx.clone(),
                        attempt,
                        combined_entry.clone(),
                        hg_repo.clone(),
                    )
                }
            },
            base_retry_delay_ms,
            retry_num,
        )
        .map(|(_, attempts)| attempts)
    })
}

/// Logs to Scuba information about a single bundle sync event
fn log_processed_entry_to_scuba(
    log_entry: &BookmarkUpdateLogEntry,
    mut scuba_sample: ScubaSampleBuilder,
    error: Option<String>,
    attempts: RetryAttemptsCount,
    duration: Duration,
    queue_size: QueueSize,
) {
    let entry = log_entry.id;
    let book = format!("{}", log_entry.bookmark_name);
    let reason = format!("{}", log_entry.reason);
    let delay = log_entry.timestamp.since_seconds();

    scuba_sample
        .add("entry", entry)
        .add("bookmark", book)
        .add("reason", reason)
        .add("attempts", attempts.0)
        .add("duration", duration.as_millis() as i64);

    match error {
        Some(error) => {
            scuba_sample.add("success", 0).add("err", error);
        }
        None => {
            scuba_sample.add("success", 1).add("delay", delay);
            scuba_sample.add("queue_size", queue_size.0);
        }
    };

    scuba_sample.log();
}

fn log_processed_entries_to_scuba(
    entries: &[BookmarkUpdateLogEntry],
    scuba_sample: ScubaSampleBuilder,
    error: Option<String>,
    attempts: RetryAttemptsCount,
    duration: Duration,
    queue_size: QueueSize,
) {
    let n: f64 = entries.len() as f64;
    let individual_duration = duration.div_f64(n);
    entries.iter().for_each(|entry| {
        log_processed_entry_to_scuba(
            entry,
            scuba_sample.clone(),
            error.clone(),
            attempts,
            individual_duration,
            queue_size,
        )
    });
}

fn get_path(f: &NamedTempFile) -> Result<String> {
    f.path()
        .to_str()
        .map(|s| s.to_string())
        .ok_or(Error::msg("non-utf8 file"))
}

fn loop_over_log_entries(
    ctx: CoreContext,
    bookmarks: impl BookmarkUpdateLog,
    repo_id: RepositoryId,
    start_id: i64,
    loop_forever: bool,
    scuba_sample: ScubaSampleBuilder,
    fetch_up_to_bundles: u64,
    repo_read_write_fetcher: RepoReadWriteFetcher,
) -> impl Stream<Item = Vec<BookmarkUpdateLogEntry>, Error = Error> {
    stream::unfold(Some(start_id), move |maybe_id| match maybe_id {
        Some(current_id) => Some(
            bookmarks
                .read_next_bookmark_log_entries_same_bookmark_and_reason(
                    ctx.clone(),
                    current_id as u64,
                    fetch_up_to_bundles,
                )
                .compat()
                .collect()
                .and_then({
                    cloned!(ctx, repo_read_write_fetcher, mut scuba_sample);
                    move |entries| match entries.iter().last().cloned() {
                        None => {
                            if loop_forever {
                                info!(ctx.logger(), "id: {}, no new entries found", current_id);
                                scuba_sample
                                    .add("repo", repo_id.id())
                                    .add("success", 1)
                                    .add("delay", 0)
                                    .log();

                                // First None means that no new entries will be added to the stream,
                                // Some(current_id) means that bookmarks will be fetched again
                                sleep(Duration::new(SLEEP_SECS, 0))
                                    .from_err()
                                    .and_then({
                                        cloned!(ctx, repo_read_write_fetcher);
                                        move |()| {
                                            unlock_repo_if_locked(ctx, repo_read_write_fetcher)
                                        }
                                    })
                                    .map(move |()| (vec![], Some(current_id)))
                                    .right_future()
                            } else {
                                ok((vec![], None)).left_future()
                            }
                        }
                        Some(last_entry) => ok((entries, Some(last_entry.id))).left_future(),
                    }
                }),
        ),
        None => None,
    })
}

#[derive(Clone)]
pub struct BookmarkOverlay {
    bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    overlay: HashMap<BookmarkName, Option<ChangesetId>>,
}

impl BookmarkOverlay {
    fn new(bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>) -> Self {
        Self {
            bookmarks,
            overlay: HashMap::new(),
        }
    }

    fn update(&mut self, book: BookmarkName, val: Option<ChangesetId>) {
        self.overlay.insert(book, val);
    }

    fn get_bookmark_values(&self) -> Vec<ChangesetId> {
        let mut res = vec![];
        for key in self.bookmarks.keys().chain(self.overlay.keys()) {
            if let Some(val) = self.overlay.get(key) {
                res.extend(val.clone().into_iter());
            } else if let Some(val) = self.bookmarks.get(key) {
                res.push(*val);
            }
        }

        res
    }
}

fn run(ctx: CoreContext, matches: ArgMatches<'static>) -> BoxFuture<(), Error> {
    let hg_repo_path = match matches.value_of("hg-repo-ssh-path") {
        Some(hg_repo_path) => hg_repo_path.to_string(),
        None => {
            error!(ctx.logger(), "Path to hg repository must be specified");
            std::process::exit(1);
        }
    };

    let log_to_scuba = matches.is_present("log-to-scuba");
    let mut scuba_sample = if log_to_scuba {
        ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
    } else {
        ScubaSampleBuilder::with_discard()
    };
    scuba_sample.add_common_server_data();

    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    let repo_id = args::get_repo_id(ctx.fb, &matches).expect("need repo id");
    let repo_config = args::get_config(ctx.fb, &matches);
    let (repo_name, repo_config) = try_boxfuture!(repo_config);

    let base_retry_delay_ms = args::get_u64_opt(&matches, "base-retry-delay-ms").unwrap_or(1000);
    let retry_num = args::get_usize(&matches, "retry-num", DEFAULT_RETRY_NUM);

    let generate_bundles = matches.is_present(GENERATE_BUNDLES);
    let bookmark_regex_force_lfs = try_boxfuture!(matches
        .value_of(ARG_BOOKMARK_REGEX_FORCE_GENERATE_LFS)
        .map(Regex::new)
        .transpose());

    let lfs_params = repo_config.lfs.clone();

    let filenode_verifier = match matches.value_of("verify-lfs-blob-presence") {
        Some(uri) => {
            let uri = try_boxfuture!(uri.parse::<Uri>());
            let verifier = try_boxfuture!(LfsVerifier::new(uri));
            FilenodeVerifier::LfsVerifier(verifier)
        }
        None => FilenodeVerifier::NoopVerifier,
    };

    let hgsql_use_sqlite = matches.is_present(HGSQL_GLOBALREVS_USE_SQLITE);
    let hgsql_db_addr = matches
        .value_of(HGSQL_GLOBALREVS_DB_ADDR)
        .map(|a| a.to_string());

    let repo_parts = args::open_repo(ctx.fb, &ctx.logger(), &matches).and_then({
        cloned!(ctx, hg_repo_path);
        let fb = ctx.fb;
        let maybe_skiplist_blobstore_key = repo_config.skiplist_index_blobstore_key.clone();
        let hgsql_globalrevs_name = repo_config.hgsql_globalrevs_name.clone();
        move |repo| {
            let overlay = list_hg_server_bookmarks(hg_repo_path.clone())
                .and_then({
                    cloned!(ctx, repo);
                    move |bookmarks| {
                        stream::iter_ok(bookmarks.into_iter())
                            .map(move |(book, hg_cs_id)| {
                                repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id).map(
                                    move |maybe_bcs_id| maybe_bcs_id.map(|bcs_id| (book, bcs_id)),
                                )
                            })
                            .buffered(100)
                            .filter_map(|x| x)
                            .collect_to::<HashMap<_, _>>()
                    }
                })
                .map(Arc::new)
                .map(BookmarkOverlay::new);

            let preparer = if generate_bundles {
                BundlePreparer::new_generate_bundles(
                    ctx,
                    repo.clone(),
                    base_retry_delay_ms,
                    retry_num,
                    maybe_skiplist_blobstore_key,
                    lfs_params,
                    filenode_verifier,
                    bookmark_regex_force_lfs,
                )
                .boxify()
            } else {
                BundlePreparer::new_use_existing(repo.clone(), base_retry_delay_ms, retry_num)
                    .boxify()
            };

            let globalrev_syncer = {
                async move {
                    if !generate_bundles && hgsql_db_addr.is_some() {
                        return Err(format_err!(
                            "Syncing globalrevs ({}) requires generating bundles ({})",
                            HGSQL_GLOBALREVS_DB_ADDR,
                            GENERATE_BUNDLES
                        ));
                    }

                    GlobalrevSyncer::new(
                        fb,
                        repo,
                        hgsql_use_sqlite,
                        hgsql_db_addr.as_ref().map(|a| a.as_ref()),
                        mysql_options,
                        readonly_storage.0,
                        hgsql_globalrevs_name,
                    )
                    .await
                }
            }
            .boxed()
            .compat();

            preparer.map(Arc::new).join3(overlay, globalrev_syncer)
        }
    });

    let batch_size = args::get_usize(&matches, "batch-size", DEFAULT_BATCH_SIZE);
    let single_bundle_timeout_ms = args::get_u64(
        &matches,
        "single-bundle-timeout-ms",
        DEFAULT_SINGLE_BUNDLE_TIMEOUT_MS,
    );
    let verify_server_bookmark_on_failure = matches.is_present("verify-server-bookmark-on-failure");
    let hg_repo = hgrepo::HgRepo::new(
        hg_repo_path,
        batch_size,
        single_bundle_timeout_ms,
        verify_server_bookmark_on_failure,
    );
    let repos = repo_parts.join(hg_repo);
    scuba_sample.add("repo", repo_id.id());
    scuba_sample.add("reponame", repo_name.clone());

    let myrouter_ready_fut = myrouter_ready(
        repo_config.primary_metadata_db_address(),
        mysql_options,
        ctx.logger().clone(),
    );
    let bookmarks = args::open_sql::<SqlBookmarksBuilder>(ctx.fb, &matches);

    myrouter_ready_fut
        .join(bookmarks)
        .and_then(move |(_, bookmarks)| {
            let bookmarks = bookmarks.with_repo_id(repo_id);
            let reporting_handler = build_reporting_handler(
                ctx.clone(),
                scuba_sample.clone(),
                retry_num,
                bookmarks.clone(),
            );

            let repo_lockers = get_read_write_fetcher(
                mysql_options,
                try_boxfuture!(get_repo_sqldb_address(&ctx, &matches, &repo_config.hgsql_name)).as_deref(),
                repo_config.hgsql_name.clone(),
                matches.is_present("lock-on-failure"),
                matches.is_present("repo-lock-sqlite"),
                readonly_storage.0,
            );

            let (lock_via, unlock_via) = try_boxfuture!(repo_lockers);

            match matches.subcommand() {
                (MODE_SYNC_ONCE, Some(sub_m)) => {
                    let start_id = try_boxfuture!(args::get_usize_opt(&sub_m, "start-id")
                        .ok_or(Error::msg("--start-id must be specified")));

                    bookmarks
                        .read_next_bookmark_log_entries(ctx.clone(), start_id as u64, 1, Freshness::MaybeStale)
                        .compat()
                        .collect()
                        .map(|entries| entries.into_iter().next())
                        .join(repos)
                        .and_then({
                            cloned!(ctx);
                            move |(maybe_log_entry, ((bundle_preparer, overlay, globalrev_syncer), hg_repo))| {
                                if let Some(log_entry) = maybe_log_entry {
                                    bundle_preparer.prepare_single_bundle(
                                        ctx.clone(),
                                        log_entry.clone(),
                                        overlay.clone(),
                                    )
                                    .and_then({
                                        cloned!(ctx);
                                        |prepared_log_entry| {
                                            combine_entries(ctx, &vec![prepared_log_entry])
                                        }
                                    })
                                    .and_then({
                                        cloned!(ctx);
                                        move |combined_entry| {
                                            sync_single_combined_entry(
                                                ctx.clone(),
                                                combined_entry,
                                                hg_repo,
                                                base_retry_delay_ms,
                                                retry_num,
                                                globalrev_syncer.clone(),
                                            )
                                        }
                                    })
                                    .then(move |r| {
                                        bind_sync_result(&vec![log_entry], r).into_future()
                                    })
                                    .collect_timing()
                                    .map_err(|(stats, e)| (Some(stats), e))
                                    .then(reporting_handler)
                                    .then(build_outcome_handler(ctx.clone(), lock_via))
                                    .map(|_| ())
                                    .left_future()
                                } else {
                                    info!(ctx.logger(), "no log entries found");
                                    Ok(()).into_future().right_future()
                                }
                            }
                        })
                        .boxify()
                }
                (MODE_SYNC_LOOP, Some(sub_m)) => {
                    let start_id = args::get_i64_opt(&sub_m, "start-id");
                    let bundle_buffer_size =
                        args::get_usize_opt(&sub_m, "bundle-prefetch").unwrap_or(0) + 1;
                    let combine_bundles = args::get_u64_opt(&sub_m, "combine-bundles").unwrap_or(1);
                    if combine_bundles != 1 {
                        panic!(
                            "For now, we don't allow combining bundles. See T43929272 for details"
                        );
                    }
                    let loop_forever = sub_m.is_present("loop-forever");
                    let mutable_counters = args::open_sql::<SqlMutableCounters>(ctx.fb, &matches);
                    let exit_path = sub_m
                        .value_of("exit-file")
                        .map(|name| Path::new(name).to_path_buf());

                    // NOTE: We poll this callback twice:
                    // - Once after possibly pulling a new piece of work.
                    // - Once after pulling a prepared piece of work.
                    //
                    // This ensures that we exit ASAP in the two following cases:
                    // - There is no work whatsoever. The first check exits early.
                    // - There is a lot of buffered work. The 2nd check exits early without doing it all.
                    let can_continue = Arc::new({
                        cloned!(ctx);
                        move || match exit_path {
                            Some(ref exit_path) if exit_path.exists() => {
                                info!(ctx.logger(), "path {:?} exists: exiting ...", exit_path);
                                false
                            }
                            _ => true,
                        }
                    });

                    mutable_counters
                        .and_then(move |mutable_counters| {
                            let counter = mutable_counters
                                .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                                .and_then(move |maybe_counter| {
                                    maybe_counter
                                        .or_else(move || start_id)
                                        .ok_or(format_err!(
                                            "{} counter not found. Pass `--start-id` flag to set the counter",
                                            LATEST_REPLAYED_REQUEST_KEY
                                        ))
                                });

                            cloned!(ctx);
                            counter
                                .join(repos)
                                .map(move |(start_id, repos)| {
                                    let ((bundle_preparer, mut overlay, globalrev_syncer), hg_repo) = repos;

                                    loop_over_log_entries(
                                        ctx.clone(),
                                        bookmarks.clone(),
                                        repo_id,
                                        start_id,
                                        loop_forever,
                                        scuba_sample.clone(),
                                        combine_bundles,
                                        unlock_via.clone(),
                                    )
                                    .take_while({
                                        cloned!(can_continue);
                                        move |_| ok(can_continue())
                                    })
                                    .filter_map(|entry_vec| {
                                        if entry_vec.len() == 0 {
                                            None
                                        } else {
                                            Some(entry_vec)
                                        }
                                    })
                                    .map_err(|cause| AnonymousError { cause })
                                    .map({
                                        cloned!(ctx, bundle_preparer);
                                        move |entries: Vec<BookmarkUpdateLogEntry>| {
                                            cloned!(ctx, bundle_preparer);
                                            let mut futs = vec![];
                                            for log_entry in entries {
                                                let f = bundle_preparer.prepare_single_bundle(
                                                    ctx.clone(),
                                                    log_entry.clone(),
                                                    overlay.clone(),
                                                );
                                                let f = spawn_future(f)
                                                    .map_err({
                                                        cloned!(log_entry);
                                                        move |err| {
                                                            bind_sync_err(&vec![log_entry], err)
                                                        }
                                                    })
                                                    // boxify is used here because of the
                                                    // type_length_limit limitation, which gets exceeded
                                                    // if we use heap-allocated types
                                                    .boxify();
                                                overlay.update(
                                                    log_entry.bookmark_name.clone(),
                                                    log_entry.to_changeset_id.clone(),
                                                );
                                                futs.push(f);
                                            }

                                            join_all(futs).and_then({
                                                cloned!(ctx);
                                                |prepared_log_entries| {
                                                    combine_entries(ctx, &prepared_log_entries)
                                                        .map_err(|e| {
                                                            bind_sync_err(
                                                                &prepared_log_entries
                                                                    .into_iter()
                                                                    .map(|prepared_entry| {
                                                                        prepared_entry.log_entry
                                                                    })
                                                                    .collect::<Vec<_>>(),
                                                                e,
                                                            )
                                                        })
                                                }
                                            })
                                        }
                                    })
                                    .buffered(bundle_buffer_size)
                                    .take_while({
                                        cloned!(can_continue);
                                        move |_| ok(can_continue())
                                    })
                                    .then({
                                        cloned!(ctx, hg_repo);

                                        move |res| match res {
                                            Ok(combined_entry) => sync_single_combined_entry(
                                                ctx.clone(),
                                                combined_entry.clone(),
                                                hg_repo.clone(),
                                                base_retry_delay_ms,
                                                retry_num,
                                                globalrev_syncer.clone(),
                                            )
                                            .then(move |r| {
                                                bind_sync_result(&combined_entry.components, r)
                                            })
                                            .collect_timing()
                                            .map_err(|(stats, e)| (Some(stats), e))
                                            .left_future(),
                                            Err(e) => err((None, e)).right_future(),
                                        }
                                    })
                                    .then(reporting_handler)
                                    .then(build_outcome_handler(ctx.clone(), lock_via))
                                    .map(move |entry| {
                                        let next_id = get_id_to_search_after(&entry);
                                        retry(
                                            ctx.logger().clone(),
                                            {
                                                cloned!(ctx, mutable_counters);
                                                move |_| {
                                                    mutable_counters.set_counter(
                                                        ctx.clone(),
                                                        repo_id,
                                                        LATEST_REPLAYED_REQUEST_KEY,
                                                        next_id,
                                                        // TODO(stash): do we need conditional updates here?
                                                        None,
                                                    )
                                                    .and_then(|success| {
                                                        if success {
                                                            Ok(())
                                                        } else {
                                                            bail!("failed to update counter")
                                                        }
                                                    })
                                                }
                                            },
                                            base_retry_delay_ms,
                                            retry_num,
                                        )
                                    })
                                })
                                .flatten_stream()
                                .for_each(|res| res.map(|_| ()))
                                .boxify()
                        })
                        .boxify()
                }
                _ => {
                    error!(ctx.logger(), "incorrect mode of operation is specified");
                    std::process::exit(1);
                }
            }
        })
        .boxify()
}

fn get_repo_sqldb_address<'a>(
    ctx: &CoreContext,
    matches: &ArgMatches<'a>,
    repo_name: &HgsqlName,
) -> Result<Option<String>, Error> {
    if let Some(db_addr) = matches.value_of("repo-lock-db-address") {
        return Ok(Some(db_addr.to_string()));
    }
    if !matches.is_present("lock-on-failure") {
        return Ok(None);
    }
    let handle = args::get_config_handle(
        ctx.fb,
        ctx.logger().clone(),
        Some(CONFIGERATOR_HGSERVER_PATH),
        1,
    )?;
    let config: Arc<ServerConfig> = handle.get();
    match config.sql_confs.get(AsRef::<str>::as_ref(repo_name)) {
        Some(sql_conf) => Ok(Some(sql_conf.db_tier.clone())),
        None => Ok(Some(config.sql_conf_default.db_tier.clone())),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = args::MononokeApp::new("Mononoke -> hg sync job")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .arg(
            Arg::with_name("hg-repo-ssh-path")
                .takes_value(true)
                .required(true)
                .help("Remote path to hg repo to replay to. Example: ssh://hg.vip.facebook.com//data/scm/fbsource"),
        )
        .arg(
            Arg::with_name("log-to-scuba")
                .long("log-to-scuba")
                .takes_value(false)
                .required(false)
                .help("If set job will log individual bundle sync states to Scuba"),
        )
        .arg(
            Arg::with_name("lock-on-failure")
                .long("lock-on-failure")
                .takes_value(false)
                .required(false)
                .help("If set, mononoke repo will be locked on sync failure"),
        )
        .arg(
            Arg::with_name("base-retry-delay-ms")
                .long("base-retry-delay-ms")
                .takes_value(true)
                .required(false)
                .help("initial delay between failures. It will be increased on the successive attempts")
        )
        .arg(
            Arg::with_name("retry-num")
                .long("retry-num")
                .takes_value(true)
                .required(false)
                .help("how many times to retry to sync a single bundle")
        )
        .arg(
            Arg::with_name("batch-size")
                .long("batch-size")
                .takes_value(true)
                .required(false)
                .help("maximum number of bundles allowed over a single hg peer")
        )
        .arg(
            Arg::with_name("single-bundle-timeout-ms")
                .long("single-bundle-timeout-ms")
                .takes_value(true)
                .required(false)
                .help("a timeout to send a single bundle to (if exceeded, the peer is restarted)")
        )
        .arg(
            Arg::with_name("verify-server-bookmark-on-failure")
                .long("verify-server-bookmark-on-failure")
                .takes_value(false)
                .required(false)
                .help("if present, check after a failure whether a server bookmark is already in the expected location")
        )
        .arg(
            Arg::with_name("repo-lock-sqlite")
                .long("repo-lock-sqlite")
                .takes_value(false)
                .required(false)
                .help("Enable sqlite for repo_lock access, path is in repo-lock-db-address"),
        )
        .arg(
            Arg::with_name("repo-lock-db-address")
                .long("repo-lock-db-address")
                .takes_value(true)
                .required(false)
                .help("Db with repo_lock table. Will be used to lock/unlock repo"),
        )
        .arg(
            Arg::with_name(HGSQL_GLOBALREVS_USE_SQLITE)
                .long(HGSQL_GLOBALREVS_USE_SQLITE)
                .takes_value(false)
                .required(false)
                .help("Use sqlite for hgsql globalrev sync (use for testing)."),
        )
        .arg(
            Arg::with_name(HGSQL_GLOBALREVS_DB_ADDR)
                .long(HGSQL_GLOBALREVS_DB_ADDR)
                .takes_value(true)
                .required(false)
                .help("Sync globalrevs to this database prior to syncing bundles."),
        )
        .arg(
            Arg::with_name(GENERATE_BUNDLES)
                .long(GENERATE_BUNDLES)
                .takes_value(false)
                .required(false)
                .help("Generate new bundles instead of using bundles that were saved on Mononoke during push"),
        )
        .arg(
            Arg::with_name(ARG_BOOKMARK_REGEX_FORCE_GENERATE_LFS)
                .long(ARG_BOOKMARK_REGEX_FORCE_GENERATE_LFS)
                .takes_value(true)
                .required(false)
                .requires(GENERATE_BUNDLES)
                .help("force generation of lfs bundles for bookmarks that match regex"),
        )
        .arg(
            Arg::with_name("verify-lfs-blob-presence")
                .long("verify-lfs-blob-presence")
                .takes_value(true)
                .required(false)
                .help("If generating bundles, verify lfs blob presence at this batch endpoint"),
        )
        .about(
            "Special job that takes bundles that were sent to Mononoke and \
             applies them to mercurial",
        );

    let sync_once = SubCommand::with_name(MODE_SYNC_ONCE)
        .about("Syncs a single bundle")
        .arg(
            Arg::with_name("start-id")
                .long("start-id")
                .takes_value(true)
                .required(true)
                .help("id in the database table to start sync with"),
        );
    let sync_loop = SubCommand::with_name(MODE_SYNC_LOOP)
        .about("Syncs bundles one by one")
        .arg(
            Arg::with_name("start-id")
                .long("start-id")
                .takes_value(true)
                .required(true)
                .help("if current counter is not set then `start-id` will be used"),
        )
        .arg(
            Arg::with_name("loop-forever")
                .long("loop-forever")
                .takes_value(false)
                .required(false)
                .help(
                    "If set job will loop forever even if there are no new entries in db or \
                     if there was an error",
                ),
        )
        .arg(
            Arg::with_name("bundle-prefetch")
                .long("bundle-prefetch")
                .takes_value(true)
                .required(false)
                .help("How many bundles to prefetch"),
        )
        .arg(
            Arg::with_name("exit-file")
                .long("exit-file")
                .takes_value(true)
                .required(false)
                .help(
                    "If you provide this argument, the sync loop will gracefully exit \
                     once this file exists",
                ),
        )
        .arg(
            Arg::with_name("combine-bundles")
                .long("combine-bundles")
                .takes_value(true)
                .required(false)
                .help("How many bundles to combine into a single bundle before sending to hg"),
        );
    let app = app.subcommand(sync_once).subcommand(sync_loop);

    let matches = app.get_matches();
    let logger = args::init_logging(fb, &matches);

    args::init_cachelib(fb, &matches, None);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    // TODO: Don't take ownership of matches here
    let fut = run(ctx.clone(), matches.clone()).compat();

    block_execute(
        fut,
        fb,
        "hg_sync_job",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

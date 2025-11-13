/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::num::NonZeroU64;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use futures_stats::FutureStats;
use itertools::Itertools;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use sql::QueryTelemetry;
use sql_query_telemetry::SqlQueryTelemetry;
use stats::prelude::*;
use strum::Display;
use strum::EnumString;

use crate::ConsistentReadError;

const SQL_TELEMETRY_SCUBA_TABLE: &str = "mononoke_sql_telemetry";

#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Copy,
    serde::Deserialize,
    EnumString,
    Display
)]
pub enum TelemetryGranularity {
    /// From a single query
    Query,
    /// From a query within a transaction
    TransactionQuery,
    /// From a transaction (i.e. when committing it)
    Transaction,
    /// A single query from a ConsistentRead operation. Similar, to TransactionQuery,
    /// this is used to track telemetry at the individual query level.
    ConsistentReadQuery,
    /// An entire ConsistentRead operation. This will aggregate telemetry for
    /// all the queries involved, e.g. retries, fallback to master.
    ConsistentRead,
}

/// Telemetry we would like to keep track of for a transaction
#[derive(Clone, Debug, Default)]
pub struct TransactionTelemetry {
    /// Tables that were read from
    pub read_tables: HashSet<String>,
    /// Tables that were written to
    pub write_tables: HashSet<String>,
    /// Repo ids that were involved in at least one query in this transaction
    pub repo_ids: HashSet<RepositoryId>,
    /// All queries that ran as part of this transaction
    pub query_names: HashSet<String>,
}

// TODO(T223577767): make these args required and remove the need for
// `log_query_telemetry_impl`
pub fn log_query_telemetry(
    opt_tel: Option<QueryTelemetry>,
    sql_query_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: &str,
    shard_name: &str,
    fut_stats: FutureStats,
) -> Result<()> {
    match opt_tel {
        Some(query_tel) => log_query_telemetry_impl(
            query_tel,
            sql_query_tel,
            granularity,
            repo_ids,
            query_name,
            shard_name,
            fut_stats,
        ),
        // TODO(T223577767): handle case when there's no telemetry
        None => Ok(()),
    }
}

// TODO(T223577767): make these args required and remove the need for
// `log_query_telemetry_impl`
pub fn log_transaction_telemetry(
    txn_tel: TransactionTelemetry,
    sql_query_tel: SqlQueryTelemetry,
    shard_name: &str,
) -> Result<()> {
    #[cfg(fbcode_build)]
    facebook::log_transaction_telemetry_impl(txn_tel, &sql_query_tel, shard_name)?;
    #[cfg(not(fbcode_build))]
    let _ = (txn_tel, sql_query_tel, shard_name);

    Ok(())
}

fn setup_error_logging(
    sql_query_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: &str,
    shard_name: &str,
    attempt: usize,
    will_retry: bool,
) -> Result<MononokeScubaSampleBuilder> {
    let jk_sample_rate = justknobs::get_as::<u64>(
        "scm/mononoke:sql_telemetry_error_sample_rate",
        Some(shard_name),
    )
    .unwrap_or(10);

    let mut scuba = setup_scuba_sample(
        sql_query_tel,
        granularity,
        repo_ids,
        Some(query_name),
        shard_name,
        jk_sample_rate,
    )?;

    scuba.add("attempt", attempt);
    scuba.add("will_retry", if will_retry { 1 } else { 0 });

    Ok(scuba)
}
/// Log query errors to Scuba on a best-effort basis.
pub fn log_query_error(
    sql_query_tel: &SqlQueryTelemetry,
    err: &Error,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: &str,
    shard_name: &str,
    attempt: usize,
    will_retry: bool,
) {
    // Also log error to the new MononokeXdbTelemetry logger
    #[cfg(fbcode_build)]
    if let Err(log_err) = facebook::log_error_to_mononoke_xdb_telemetry_logger(
        sql_query_tel.fb().clone(),
        sql_query_tel,
        granularity,
        repo_ids,
        query_name,
        shard_name,
        err,
        attempt,
        will_retry,
    ) {
        tracing::error!("Failed to log error to MononokeXDBTelemetry logger: {log_err:?}");
    }
    let mut scuba = match setup_error_logging(
        sql_query_tel,
        granularity,
        repo_ids,
        query_name,
        shard_name,
        attempt,
        will_retry,
    ) {
        Ok(scuba) => scuba,
        // This is the only call that can return an Err, but errors will be
        // ignored and logged to stderr instead.
        Err(e) => {
            tracing::error!("Failed to setup scuba sample: {e}");
            return;
        }
    };

    scuba.add("error", format!("{:?}", err));
    scuba.add("success", 0);

    #[cfg(fbcode_build)]
    facebook::handle_mysql_error(err, shard_name, query_name, attempt);

    // Log the Scuba sample for debugging when log-level is set to trace.
    tracing::trace!(
        "Logging query telemetry to scuba: {0:#?}",
        scuba.get_sample()
    );

    scuba.log();
}

pub fn log_consistent_read_query_error(
    sql_query_tel: &SqlQueryTelemetry,
    cons_read_err: &ConsistentReadError,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: &str,
    shard_name: &str,
    attempt: usize,
    will_retry: bool,
) {
    match cons_read_err {
        ConsistentReadError::QueryError(err) => {
            // Underlying query errors are treated the same as other queries.
            STATS::replica_lagging.add_value(1, (shard_name.to_string(), query_name.to_string()));
            return log_query_error(
                sql_query_tel,
                err,
                granularity,
                repo_ids,
                query_name,
                shard_name,
                attempt,
                will_retry,
            );
        }
        ConsistentReadError::ReplicaLagging => {
            STATS::replica_lagging.add_value(1, (shard_name.to_string(), query_name.to_string()));
        }
        ConsistentReadError::MissingHLC => {
            STATS::missing_hlc.add_value(1, (shard_name.to_string(), query_name.to_string()));
        }
    };

    let mut scuba = match setup_error_logging(
        sql_query_tel,
        granularity,
        repo_ids,
        query_name,
        shard_name,
        attempt,
        will_retry,
    ) {
        Ok(scuba) => scuba,
        // This is the only call that can return an Err, but errors will be
        // ignored and logged to stderr instead.
        Err(e) => {
            tracing::error!("Failed to setup scuba sample: {e}");
            return;
        }
    };

    scuba.add("success", 1);
    STATS::success.add_value(1, (shard_name.to_string(),));
    STATS::success_query.add_value(1, (shard_name.to_string(), query_name.to_string()));

    // Log the Scuba sample for debugging when log-level is set to trace.
    tracing::trace!(
        "Logging query telemetry to scuba: {0:#?}",
        scuba.get_sample()
    );

    scuba.log();
}

fn log_query_telemetry_impl(
    query_tel: QueryTelemetry,
    sql_query_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: &str,
    shard_name: &str,
    fut_stats: FutureStats,
) -> Result<()> {
    #[cfg(not(fbcode_build))]
    {
        // To remove typechecker unused variable warning in OSS
        let _ = (
            sql_query_tel,
            granularity,
            repo_ids,
            query_name,
            shard_name,
            fut_stats,
        );
    }
    match query_tel {
        #[cfg(fbcode_build)]
        QueryTelemetry::MySQL(telemetry) => {
            // TODO(T223577767): ensure MySQL always has shard_name

            // Log to scuba
            facebook::log_mysql_query_telemetry(
                sql_query_tel.fb().clone(),
                telemetry,
                sql_query_tel,
                granularity,
                repo_ids,
                query_name,
                shard_name,
                fut_stats,
            )
        }
        QueryTelemetry::Sqlite(_) => Ok(()),
        _ => Err(anyhow!("Unsupported query telemetry type")),
    }
}

/// Sets fields that are present in both successful and failed queries.
fn setup_scuba_sample(
    sql_query_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: &[RepositoryId],
    query_name: Option<&str>,
    shard_name: &str,
    sample_rate: u64,
) -> Result<MononokeScubaSampleBuilder> {
    let fb = sql_query_tel.fb().clone();

    // Log to file if SQL_TELEMETRY_SCUBA_FILE_PATH is set (for testing)
    let mut scuba = if let Ok(scuba_file_path) = std::env::var("SQL_TELEMETRY_SCUBA_FILE_PATH") {
        MononokeScubaSampleBuilder::with_discard().with_log_file(scuba_file_path)?
    } else {
        MononokeScubaSampleBuilder::new(fb, SQL_TELEMETRY_SCUBA_TABLE)?
    };

    scuba.add_metadata(sql_query_tel.metadata());

    scuba.add_common_server_data();

    scuba.add("granularity", format!("{:?}", granularity));
    scuba.add("query_name", query_name);
    scuba.add("shard_name", shard_name);

    match NonZeroU64::new(sample_rate).ok_or(anyhow!("Sample rate must be a positive number")) {
        Ok(sample_rate) => {
            scuba.sampled(sample_rate);
        }
        Err(e) => {
            tracing::error!("Failed to set Scuba sample rate from JustKnobs: {e:?}");
        }
    };

    scuba.add(
        "repo_ids",
        // Scuba only supports NormVector of Strings
        repo_ids
            .into_iter()
            .sorted()
            .dedup()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
    );

    Ok(scuba)
}

impl TransactionTelemetry {
    pub fn add_query_telemetry(&mut self, query_tel: QueryTelemetry) {
        match query_tel {
            #[cfg(fbcode_build)]
            QueryTelemetry::MySQL(mysql_tel) => {
                facebook::add_mysql_query_telemetry(self, mysql_tel);
            }
            _ => (),
        }
    }

    pub fn add_repo_ids<I>(&mut self, repo_ids: I)
    where
        I: IntoIterator<Item = RepositoryId>,
    {
        self.repo_ids.extend(repo_ids);
    }

    pub fn add_query_name(&mut self, query_name: &str) {
        self.query_names.insert(query_name.to_string());
    }
}

#[cfg(fbcode_build)]
mod facebook {
    use anyhow::Result;
    use fbinit::FacebookInit;
    use futures_stats::FutureStats;
    use itertools::Itertools;
    use mononoke_types::RepositoryId;
    use mononoke_xdb_telemetry_logger::MononokeXdbTelemetryLogger;
    use mysql_client::MysqlError;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql::mysql::MysqlQueryTelemetry;
    use sql_query_telemetry::SqlQueryTelemetry;
    use stats::prelude::*;

    use super::STATS;
    use super::TelemetryGranularity;
    use super::TransactionTelemetry;
    use super::setup_scuba_sample;

    pub(super) fn log_mysql_query_telemetry(
        fb: FacebookInit,
        query_tel: MysqlQueryTelemetry,
        sql_query_tel: &SqlQueryTelemetry,
        granularity: TelemetryGranularity,
        repo_ids: &[RepositoryId],
        query_name: &str,
        shard_name: &str,
        fut_stats: FutureStats,
    ) -> Result<()> {
        // Also log to the new MononokeXDBTelemetry logger
        if let Err(e) = log_to_mononoke_xdb_telemetry_logger(
            fb,
            query_tel.clone(),
            sql_query_tel,
            granularity,
            repo_ids,
            query_name,
            shard_name,
            fut_stats.clone(),
        ) {
            tracing::error!("Failed to log to MononokeXDBTelemetry logger: {e:?}");
        }

        let jk_sample_rate =
            justknobs::get_as::<u64>("scm/mononoke:sql_telemetry_sample_rate", Some(shard_name))
                .unwrap_or(10);

        let mut scuba = setup_scuba_sample(
            sql_query_tel,
            granularity,
            repo_ids,
            Some(query_name),
            shard_name,
            jk_sample_rate,
        )?;

        scuba.add("success", 1);
        STATS::success.add_value(1, (shard_name.to_string(),));
        STATS::success_query.add_value(1, (shard_name.to_string(), query_name.to_string()));

        scuba.add_future_stats(&fut_stats);

        STATS::query_completion_time.add_value(
            fut_stats.completion_time.as_micros() as i64,
            (
                shard_name.to_string(),
                query_name.to_string(),
                format!("{:?}", granularity),
            ),
        );

        let opt_instance_type = query_tel.instance_type().cloned();

        let read_tables = query_tel.read_tables().iter().collect::<Vec<_>>();
        let write_tables = query_tel.write_tables().iter().collect::<Vec<_>>();

        let read_or_write = if write_tables.is_empty() {
            "READ"
        } else {
            "WRITE"
        };

        scuba.add("instance_type", opt_instance_type.clone());

        scuba.add("read_or_write", read_or_write.to_string());

        if let Some(instance_type) = opt_instance_type {
            // Success
            STATS::success_instance.add_value(1, (shard_name.to_string(), instance_type.clone()));

            STATS::query_instance_completion_time.add_value(
                fut_stats.completion_time.as_micros() as i64,
                (
                    shard_name.to_string(),
                    query_name.to_string(),
                    format!("{:?}", granularity),
                    instance_type.clone(),
                    read_or_write.to_string(),
                ),
            );

            // CPU and Delay RRU by instance type
            if let Some(client_stats) = query_tel.client_stats() {
                STATS::cpu_rru_instance.add_value(
                    (1000.0 * client_stats.cpu_rru) as i64,
                    (shard_name.to_string(), instance_type.clone()),
                );
                STATS::delay_rru_instance.add_value(
                    (1000.0 * client_stats.delay_rru) as i64,
                    (shard_name.to_string(), instance_type.clone()),
                );
                STATS::task_full_delay_rru_instance.add_value(
                    (1000.0 * client_stats.task_full_delay_rru) as i64,
                    (shard_name.to_string(), instance_type.clone()),
                );
                STATS::task_some_delay_rru.add_value(
                    (1000.0 * client_stats.task_some_delay_rru) as i64,
                    (shard_name.to_string(), instance_type.clone()),
                );
            };

            // Table stats
            read_tables.iter().sorted().for_each(|&table| {
                STATS::read_tables.add_value(
                    1,
                    (shard_name.to_string(), table.clone(), instance_type.clone()),
                )
            });

            write_tables.iter().sorted().for_each(|&table| {
                STATS::write_tables.add_value(
                    1,
                    (shard_name.to_string(), table.clone(), instance_type.clone()),
                )
            });
        }

        // CPU and Delay RRU by instance type
        if let Some(client_stats) = query_tel.client_stats() {
            STATS::cpu_rru.add_value(
                (1000.0 * client_stats.cpu_rru) as i64,
                (shard_name.to_string(),),
            );
            STATS::delay_rru.add_value(
                (1000.0 * client_stats.delay_rru) as i64,
                (shard_name.to_string(),),
            );
            STATS::full_delay_rru.add_value(
                (1000.0 * client_stats.full_delay_rru) as i64,
                (shard_name.to_string(),),
            );
            STATS::max_cpu_rru.add_value(
                (1000.0 * client_stats.max_rru) as i64,
                (shard_name.to_string(),),
            );
        };

        scuba.add("read_tables", read_tables);
        scuba.add("write_tables", write_tables);

        for wait_stats in query_tel.wait_stats() {
            STATS::wait_count.add_value(
                wait_stats.wait_count as i64,
                (shard_name.to_string(), wait_stats.wait_type.clone()),
            );
            wait_stats.wait_time.inspect(|wt| {
                STATS::wait_time.add_value(
                    *wt as i64,
                    (shard_name.to_string(), wait_stats.wait_type.clone()),
                );
            });

            wait_stats.signal_time.inspect(|st| {
                STATS::signal_time.add_value(
                    *st as i64,
                    (shard_name.to_string(), wait_stats.wait_type.clone()),
                );
            });

            scuba.add(
                format!("wait_count_{}", wait_stats.wait_type),
                wait_stats.wait_count,
            );
            scuba.add(
                format!("wait_time_{}", wait_stats.wait_type),
                wait_stats.wait_time,
            );
            scuba.add(
                format!("signal_time_{}", wait_stats.wait_type),
                wait_stats.signal_time,
            );
        }

        if let Some(client_stats) = query_tel.client_stats() {
            scuba.add("avg_rru", client_stats.avg_rru);
            scuba.add("cpu_rru", client_stats.cpu_rru);
            scuba.add("delay_rru", client_stats.delay_rru);
            scuba.add("full_delay_rru", client_stats.full_delay_rru);
            scuba.add("max_rru", client_stats.max_rru);
            scuba.add("min_rru", client_stats.min_rru);
            scuba.add("overlimit_delay_rru", client_stats.overlimit_delay_rru);
            scuba.add("some_delay_rru", client_stats.some_delay_rru);
            scuba.add("task_full_delay_rru", client_stats.task_full_delay_rru);
            scuba.add("task_some_delay_rru", client_stats.task_some_delay_rru);
        }

        // Log the Scuba sample for debugging when log-level is set to trace.
        tracing::trace!(
            "Logging query telemetry to scuba: {0:#?}",
            scuba.get_sample()
        );

        scuba.log();

        Ok(())
    }

    pub(super) fn setup_logger_entry(
        fb: FacebookInit,
        granularity: TelemetryGranularity,
        repo_ids: &[RepositoryId],
        query_name: &str,
        shard_name: &str,
        sql_query_tel: &SqlQueryTelemetry,
    ) -> MononokeXdbTelemetryLogger {
        // Create logger instance
        let mut log_entry = MononokeXdbTelemetryLogger::new(fb);

        // Set required fields
        log_entry.set_granularity(format!("{:?}", granularity));
        log_entry.set_shard_name(shard_name.to_string());

        // Set optional fields if available
        if !query_name.is_empty() {
            log_entry.set_query_name(query_name.to_string());
        }

        // Set repo IDs
        let repo_id_strings: Vec<String> = repo_ids.iter().map(|id| id.to_string()).collect();
        log_entry.set_repo_ids(repo_id_strings);

        // Set client request info
        set_client_request_info(&mut log_entry, sql_query_tel);

        // Set metadata fields
        set_metadata(&mut log_entry, sql_query_tel);

        log_entry
    }

    pub(super) fn log_to_mononoke_xdb_telemetry_logger(
        fb: FacebookInit,
        query_tel: MysqlQueryTelemetry,
        sql_query_tel: &SqlQueryTelemetry,
        granularity: TelemetryGranularity,
        repo_ids: &[RepositoryId],
        query_name: &str,
        shard_name: &str,
        fut_stats: FutureStats,
    ) -> Result<()> {
        let mut log_entry = setup_logger_entry(
            fb,
            granularity,
            repo_ids,
            query_name,
            shard_name,
            sql_query_tel,
        );

        log_entry.set_success(1); // This function is only called for successful queries

        if let Some(instance_type) = query_tel.instance_type() {
            log_entry.set_instance_type(instance_type.clone());
        }
        // Set table access information
        let read_tables: Vec<String> = query_tel.read_tables().iter().cloned().collect();
        let write_tables: Vec<String> = query_tel.write_tables().iter().cloned().collect();
        let read_or_write = if write_tables.is_empty() {
            "READ"
        } else {
            "WRITE"
        };
        log_entry.set_read_or_write(read_or_write.to_string());

        log_entry.set_read_tables(read_tables);
        log_entry.set_write_tables(write_tables);

        // Set resource usage fields if available
        if let Some(client_stats) = query_tel.client_stats() {
            log_entry.set_avg_rru(client_stats.avg_rru);
            log_entry.set_cpu_rru(client_stats.cpu_rru);
            log_entry.set_delay_rru(client_stats.delay_rru);
            log_entry.set_full_delay_rru(client_stats.full_delay_rru);
            log_entry.set_max_rru(client_stats.max_rru);
            log_entry.set_min_rru(client_stats.min_rru);
            log_entry.set_overlimit_delay_rru(client_stats.overlimit_delay_rru);
            log_entry.set_some_delay_rru(client_stats.some_delay_rru);
            log_entry.set_task_full_delay_rru(client_stats.task_full_delay_rru);
            log_entry.set_task_some_delay_rru(client_stats.task_some_delay_rru);
        }

        // Set wait statistics
        for wait_stats in query_tel.wait_stats() {
            match wait_stats.wait_type.as_str() {
                "row_lock" => {
                    log_entry.set_wait_count_row_lock(wait_stats.wait_count as i64);
                    if let Some(wait_time) = wait_stats.wait_time {
                        log_entry.set_wait_time_row_lock(wait_time as f64);
                    }
                    if let Some(signal_time) = wait_stats.signal_time {
                        log_entry.set_signal_time_row_lock(signal_time as f64);
                    }
                }
                "table_lock" => {
                    log_entry.set_wait_count_table_lock(wait_stats.wait_count as i64);
                    if let Some(wait_time) = wait_stats.wait_time {
                        log_entry.set_wait_time_table_lock(wait_time as f64);
                    }
                    if let Some(signal_time) = wait_stats.signal_time {
                        log_entry.set_signal_time_table_lock(signal_time as f64);
                    }
                }
                "metadata_lock" => {
                    log_entry.set_wait_count_metadata_lock(wait_stats.wait_count as i64);
                    if let Some(wait_time) = wait_stats.wait_time {
                        log_entry.set_wait_time_metadata_lock(wait_time as f64);
                    }
                    if let Some(signal_time) = wait_stats.signal_time {
                        log_entry.set_signal_time_metadata_lock(signal_time as f64);
                    }
                }
                "global_read_lock" => {
                    log_entry.set_wait_count_global_read_lock(wait_stats.wait_count as i64);
                    if let Some(wait_time) = wait_stats.wait_time {
                        log_entry.set_wait_time_global_read_lock(wait_time as f64);
                    }
                    if let Some(signal_time) = wait_stats.signal_time {
                        log_entry.set_signal_time_global_read_lock(signal_time as f64);
                    }
                }
                "backup_lock" => {
                    log_entry.set_wait_count_backup_lock(wait_stats.wait_count as i64);
                    if let Some(wait_time) = wait_stats.wait_time {
                        log_entry.set_wait_time_backup_lock(wait_time as f64);
                    }
                    if let Some(signal_time) = wait_stats.signal_time {
                        log_entry.set_signal_time_backup_lock(signal_time as f64);
                    }
                }
                _ => {
                    // Unknown wait type, skip
                }
            }
        }

        // Set future stats fields
        log_entry.set_poll_count(fut_stats.poll_count as i64);
        log_entry.set_poll_time_us(fut_stats.poll_time.as_micros() as i64);
        log_entry.set_max_poll_time_us(fut_stats.max_poll_time.as_micros() as i64);
        log_entry.set_completion_time_us(fut_stats.completion_time.as_micros() as i64);

        // Log the entry
        log_entry.log()?;

        Ok(())
    }

    pub(super) fn log_error_to_mononoke_xdb_telemetry_logger(
        fb: FacebookInit,
        sql_query_tel: &SqlQueryTelemetry,
        granularity: TelemetryGranularity,
        repo_ids: &[RepositoryId],
        query_name: &str,
        shard_name: &str,
        err: &anyhow::Error,
        attempt: usize,
        will_retry: bool,
    ) -> Result<()> {
        let mut log_entry = setup_logger_entry(
            fb,
            granularity,
            repo_ids,
            query_name,
            shard_name,
            sql_query_tel,
        );

        log_entry.set_success(0); // 0 indicates failed query
        // Set error message
        log_entry.set_error(format!("{:?}", err));
        // Set retry fields
        log_entry.set_attempt(attempt as i64);
        log_entry.set_will_retry(will_retry);

        // Log the entry
        log_entry.log()?;

        Ok(())
    }

    pub(super) fn log_transaction_telemetry_to_mononoke_xdb_telemetry_logger(
        fb: FacebookInit,
        txn_tel: &TransactionTelemetry,
        sql_query_tel: &SqlQueryTelemetry,
        shard_name: &str,
    ) -> Result<()> {
        // Convert repo_ids from HashSet to Vec for setup_logger_entry
        let repo_ids: Vec<RepositoryId> = txn_tel.repo_ids.iter().cloned().collect();

        let mut log_entry = setup_logger_entry(
            fb,
            TelemetryGranularity::Transaction,
            &repo_ids,
            "", // No single query name for transactions
            shard_name,
            sql_query_tel,
        );

        // Set transaction-specific fields
        log_entry.set_success(1); // Transactions that reach this function are successful

        // Set table access information
        let read_tables: Vec<String> = txn_tel.read_tables.iter().cloned().collect();
        let write_tables: Vec<String> = txn_tel.write_tables.iter().cloned().collect();
        log_entry.set_read_tables(read_tables);
        log_entry.set_write_tables(write_tables);

        // Set read_or_write field based on table access pattern
        let read_or_write = if txn_tel.write_tables.is_empty() {
            "READ"
        } else {
            "WRITE"
        };
        log_entry.set_read_or_write(read_or_write.to_string());

        // Set transaction query names
        let query_names: Vec<String> = txn_tel.query_names.iter().cloned().collect();
        log_entry.set_transaction_query_names(query_names);

        // Log the entry
        log_entry.log()?;

        Ok(())
    }

    pub(super) fn log_transaction_telemetry_impl(
        txn_tel: TransactionTelemetry,
        sql_query_tel: &SqlQueryTelemetry,
        shard_name: &str,
    ) -> Result<()> {
        if let Err(e) = log_transaction_telemetry_to_mononoke_xdb_telemetry_logger(
            sql_query_tel.fb().clone(),
            &txn_tel,
            sql_query_tel,
            shard_name,
        ) {
            tracing::error!("Failed to log transaction to MononokeXDBTelemetry logger: {e:?}");
        }

        let jk_sample_rate =
            justknobs::get_as::<u64>("scm/mononoke:sql_telemetry_sample_rate", Some(shard_name))
                .unwrap_or(10);

        let mut scuba = setup_scuba_sample(
            sql_query_tel,
            TelemetryGranularity::Transaction,
            &txn_tel.repo_ids.into_iter().collect::<Vec<_>>(),
            None,
            shard_name,
            jk_sample_rate,
        )?;

        scuba.add("success", 1);

        scuba.add(
            "read_tables",
            txn_tel.read_tables.into_iter().sorted().collect::<Vec<_>>(),
        );
        scuba.add(
            "write_tables",
            txn_tel
                .write_tables
                .into_iter()
                .sorted()
                .collect::<Vec<_>>(),
        );

        scuba.add(
            "transaction_query_names",
            txn_tel.query_names.into_iter().sorted().collect::<Vec<_>>(),
        );

        // Log the Scuba sample for debugging when log-level is set to trace.
        tracing::trace!(
            "Logging transaction telemetry to scuba: {0:#?}",
            scuba.get_sample()
        );

        scuba.log();

        Ok(())
    }

    /// Set client request info fields on the logger from metadata.
    fn set_client_request_info(
        log_entry: &mut MononokeXdbTelemetryLogger,
        sql_query_tel: &SqlQueryTelemetry,
    ) {
        let metadata = sql_query_tel.metadata();
        if let Some(client_info) = metadata.client_request_info() {
            if let Some(main_id) = &client_info.main_id {
                log_entry.set_client_main_id(main_id.clone());
            }
            log_entry.set_client_entry_point(client_info.entry_point.to_string());
            log_entry.set_client_correlator(client_info.correlator.clone());

            let experiments = MononokeScubaSampleBuilder::get_enabled_experiments_jk(client_info);
            if !experiments.is_empty() {
                log_entry.set_enabled_experiments_jk(experiments);
            }
        }
    }

    /// Set metadata fields on the logger from metadata.
    fn set_metadata(log_entry: &mut MononokeXdbTelemetryLogger, sql_query_tel: &SqlQueryTelemetry) {
        let metadata = sql_query_tel.metadata();

        // Session UUID
        log_entry.set_session_uuid(metadata.session_id().to_string());

        // Client identities
        let client_identities: Vec<String> = metadata
            .identities()
            .iter()
            .map(|i| i.to_string())
            .collect();
        log_entry.set_client_identities(client_identities);

        // Client identity variant
        if let Some(first_identity) = metadata.identities().first() {
            log_entry.set_client_identity_variant(first_identity.variant().to_string());
        }

        // Source hostname or client IP (mutually exclusive)
        if let Some(client_hostname) = metadata.client_hostname() {
            log_entry.set_source_hostname(client_hostname.to_owned());
        } else if let Some(client_ip) = metadata.client_ip() {
            log_entry.set_client_ip(client_ip.to_string());
        }

        // Unix username
        if let Some(unix_name) = metadata.unix_name() {
            log_entry.set_unix_username(unix_name.to_string());
        }

        // Sandcastle fields
        if let Some(sandcastle_alias) = metadata.sandcastle_alias() {
            log_entry.set_sandcastle_alias(sandcastle_alias.to_string());
        }
        if let Some(sandcastle_vcs) = metadata.sandcastle_vcs() {
            log_entry.set_sandcastle_vcs(sandcastle_vcs.to_string());
        }
        if let Some(sandcastle_nonce) = metadata.sandcastle_nonce() {
            log_entry.set_sandcastle_nonce(sandcastle_nonce.to_string());
        }

        // Reverse proxy region
        if let Some(revproxy_region) = metadata.revproxy_region() {
            log_entry.set_revproxy_region(revproxy_region.to_string());
        }

        // Tupperware client info
        if let Some(client_tw_job) = metadata.clientinfo_tw_job() {
            log_entry.set_client_tw_job(client_tw_job.to_string());
        }
        if let Some(client_tw_task) = metadata.clientinfo_tw_task() {
            log_entry.set_client_tw_task(client_tw_task.to_string());
        }

        // Atlas client info
        if let Some(client_atlas) = metadata.clientinfo_atlas() {
            log_entry.set_client_atlas(client_atlas.to_string());
        }
        if let Some(client_atlas_env_id) = metadata.clientinfo_atlas_env_id() {
            log_entry.set_client_atlas_env_id(client_atlas_env_id.to_string());
        }

        // Fetch fields
        if let Some(fetch_cause) = metadata.fetch_cause() {
            log_entry.set_fetch_cause(fetch_cause.to_string());
        }
        log_entry.set_fetch_from_cas_attempted(metadata.fetch_from_cas_attempted());

        // Common server data fields
        set_common_server_data(log_entry);
    }

    /// Set common server data fields on the logger.
    /// This replicates the logic from `add_mapped_common_server_data` in scuba/builder.rs
    fn set_common_server_data(log_entry: &mut MononokeXdbTelemetryLogger) {
        use std::env::var;

        // Server hostname
        if let Ok(hostname) = hostname::get_hostname() {
            log_entry.set_server_hostname(hostname);
        }

        // Region, datacenter, and region_datacenter_prefix from fbwhoami
        if let Ok(who) = fbwhoami::FbWhoAmI::get() {
            if let Some(region) = &who.region {
                log_entry.set_region(region.to_string());
            }
            if let Some(dc) = &who.datacenter {
                log_entry.set_datacenter(dc.to_string());
            }
            if let Some(dc_prefix) = &who.region_datacenter_prefix {
                log_entry.set_region_datacenter_prefix(dc_prefix.to_string());
            }
        }

        // Server tier from SMC_TIERS environment variable
        if let Ok(smc_tier) = var("SMC_TIERS") {
            log_entry.set_server_tier(smc_tier);
        }

        // Tupperware task ID
        if let Ok(tw_task_id) = var("TW_TASK_ID") {
            log_entry.set_tw_task_id(tw_task_id);
        }

        // Tupperware canary ID
        if let Ok(tw_canary_id) = var("TW_CANARY_ID") {
            log_entry.set_tw_canary_id(tw_canary_id);
        }

        // Chronos fields
        if let Ok(cluster) = var("CHRONOS_CLUSTER") {
            log_entry.set_chronos_cluster(cluster);
        }
        if let Ok(id) = var("CHRONOS_JOB_INSTANCE_ID") {
            log_entry.set_chronos_job_instance_id(id);
        }
        if let Ok(job_name) = var("CHRONOS_JOB_NAME") {
            log_entry.set_chronos_job_name(job_name);
        }

        // Tupperware job handle (format: cluster/user/name)
        if let (Ok(tw_cluster), Ok(tw_user), Ok(tw_name)) = (
            var("TW_JOB_CLUSTER"),
            var("TW_JOB_USER"),
            var("TW_JOB_NAME"),
        ) {
            log_entry.set_tw_handle(format!("{}/{}/{}", tw_cluster, tw_user, tw_name));
        }

        // Tupperware task handle (format: cluster/user/name/taskid)
        if let (Ok(tw_cluster), Ok(tw_user), Ok(tw_name), Ok(tw_task_id)) = (
            var("TW_JOB_CLUSTER"),
            var("TW_JOB_USER"),
            var("TW_JOB_NAME"),
            var("TW_TASK_ID"),
        ) {
            log_entry.set_tw_task_handle(format!(
                "{}/{}/{}/{}",
                tw_cluster, tw_user, tw_name, tw_task_id
            ));
        }

        // Build info (Linux only)
        #[cfg(target_os = "linux")]
        {
            log_entry.set_build_revision(build_info::BuildInfo::get_revision().to_string());
            log_entry.set_build_rule(build_info::BuildInfo::get_rule().to_string());
        }
    }

    pub(super) fn handle_mysql_error(
        e: &anyhow::Error,
        shard_name: &str,
        query_name: &str,
        attempt: usize,
    ) {
        if let Some(e) = e.downcast_ref::<MysqlError>() {
            // Get just the enum variant name using std::any::type_name
            let error_type = std::any::type_name_of_val(e)
                .split("::")
                .last()
                .unwrap_or("Unknown");

            let error_key = if let Some(mysql_errno) = e.mysql_errno() {
                format!("{error_type}.{mysql_errno}")
            } else {
                error_type.to_string()
            };
            STATS::query_retry_attempts.add_value(
                attempt as i64,
                (shard_name.to_string(), query_name.to_string(), error_key),
            );
        }
    }

    pub(super) fn add_mysql_query_telemetry(
        txn_tel: &mut TransactionTelemetry,
        query_tel: MysqlQueryTelemetry,
    ) {
        txn_tel.read_tables.extend(query_tel.read_tables);
        txn_tel.write_tables.extend(query_tel.write_tables);
    }
}

// Documentation of MySQL Client Logs: https://fburl.com/wiki/e21tf16l
define_stats! {
    prefix = "mononoke.sql_telemetry";
    success: dynamic_timeseries("{}.success", (shard_name: String); Sum, Average),
    success_instance: dynamic_timeseries(
        "{}.success.instance.{}", (shard_name: String, instance_type: String);
        Sum, Average
    ),
    success_query: dynamic_timeseries(
        "{}.success.query.{}", (shard_name: String, query_name: String);
        Sum, Average, Count
    ),
    query_retry_attempts: dynamic_timeseries(
        "{}.{}.{}.retry_attempts",
        (shard_name: String, query_name: String, error_key: String);
        Sum, Average, Count;
    ),

    query_completion_time: dynamic_timeseries(
        "{}.query.{}.granularity.{}.completion_time_us",
        (shard_name: String, query_name: String, granularity: String);
        Sum, Average
    ),

    query_instance_completion_time: dynamic_timeseries(
        "{}.query.{}.granularity.{}.instance_type.{}.{}.completion_time_us",
        (
            shard_name: String,
            query_name: String,
            granularity: String,
            instance_type: String,
            read_or_write: String
        );
        Sum, Average
    ),


    replica_lagging: dynamic_timeseries(
        "{}.{}.consistent_read.replica_lagging",
        (shard_name: String, query_name: String);
        Sum, Average, Count;
    ),
    missing_hlc: dynamic_timeseries(
        "{}.{}.consistent_read.missing_hlc",
        (shard_name: String, query_name: String);
        Sum, Average, Count;
    ),

    // Wait stats
    wait_count: dynamic_timeseries(
        "{}.wait_count.{}", (shard_name: String, wait_event_type: String);
        Sum, Average
    ),
    wait_time: dynamic_timeseries(
        "{}.wait_time.{}", (shard_name: String, wait_event_type: String);
        Sum, Average
    ),
    signal_time: dynamic_timeseries(
        "{}.signal_time.{}", (shard_name: String, wait_event_type: String);
         Sum, Average
    ),

    // CPU and Delay RRU for all tasks
    cpu_rru: dynamic_timeseries("{}.cpu_milli_rru", (shard_name: String); Sum, Average),
    max_cpu_rru: dynamic_timeseries("{}.max_cpu_milli_rru", (shard_name: String); Sum),
    delay_rru: dynamic_timeseries("{}.delay_milli_rru", (shard_name: String); Sum, Average),
    full_delay_rru: dynamic_timeseries("{}.full_delay_milli_rru", (shard_name: String);  Sum, Average),

    // CPU and Delay RRU split by instance type (e.g. Primary or Secondary)
    cpu_rru_instance: dynamic_timeseries(
        "{}.cpu_milli_rru.instance.{}",
        (shard_name: String, instance_type: String);
         Sum, Average
    ),
    delay_rru_instance: dynamic_timeseries(
        "{}.delay_milli_rru.instance.{}", (shard_name: String, instance_type: String);
         Sum, Average
    ),
    task_full_delay_rru_instance: dynamic_timeseries(
        "{}.task_full_delay_milli_rru.instance.{}", (shard_name: String, instance_type: String);
         Sum, Average
    ),
    task_some_delay_rru: dynamic_timeseries(
        "{}.task_some_delay_milli_rru.instance.{}", (shard_name: String, instance_type: String);
         Sum, Average
    ),

    // Table stats
    read_tables: dynamic_timeseries(
        "{}.reads.{}.instance.{}",
        (shard_name: String, table: String, instance_type: String);
        Count, Sum
    ),
    write_tables: dynamic_timeseries(
        "{}.writes.{}.instance.{}",
        (shard_name: String, table: String, instance_type: String);
        Count, Sum
    ),

}

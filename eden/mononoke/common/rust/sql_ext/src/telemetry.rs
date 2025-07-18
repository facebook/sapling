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
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use sql::QueryTelemetry;
#[cfg(fbcode_build)]
use sql::mysql::MysqlQueryTelemetry;
use sql_query_telemetry::SqlQueryTelemetry;
use stats::prelude::*;

const SQL_TELEMETRY_SCUBA_TABLE: &str = "mononoke_sql_telemetry";

#[derive(Clone, Debug, Eq, PartialEq, Copy, serde::Deserialize)]
pub enum TelemetryGranularity {
    /// From a single query
    Query,
    /// From a query within a transaction
    TransactionQuery,
    /// From a transaction (i.e. when committing it)
    Transaction,
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
    // TODO(T223577767): track name of the queries from the transaction
}

// TODO(T223577767): make these args required and remove the need for
// `log_query_telemetry_impl`
pub fn log_query_telemetry(
    opt_tel: Option<QueryTelemetry>,
    opt_sql_tel: Option<&SqlQueryTelemetry>,
    granularity: TelemetryGranularity,
    repo_ids: Vec<RepositoryId>,
    query_name: &str,
) -> Result<()> {
    match (opt_tel, opt_sql_tel) {
        (Some(query_tel), Some(sql_tel)) => {
            log_query_telemetry_impl(query_tel, sql_tel, granularity, repo_ids, query_name)
        }
        // TODO(T223577767): handle case when there's no telemetry
        _ => Ok(()),
    }
}

// TODO(T223577767): make these args required and remove the need for
// `log_query_telemetry_impl`
pub fn log_transaction_telemetry(
    txn_tel: TransactionTelemetry,
    sql_tel: SqlQueryTelemetry,
) -> Result<()> {
    log_transaction_telemetry_impl(txn_tel, &sql_tel)
}

/// Log query errors to Scuba on a best-effort basis.
pub fn log_query_error(
    opt_tel: Option<&SqlQueryTelemetry>,
    err: &Error,
    granularity: TelemetryGranularity,
    repo_ids: Vec<RepositoryId>,
    query_name: &str,
) {
    let sql_tel = match opt_tel.as_ref() {
        Some(sql_tel) => sql_tel,
        None => return,
    };

    let mut scuba = match setup_scuba_sample(sql_tel, granularity, repo_ids, Some(query_name)) {
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
    STATS::error.add_value(1);

    // Log the Scuba sample for debugging when log-level is set to trace.
    tracing::trace!(
        "Logging query telemetry to scuba: {0:#?}",
        scuba.get_sample()
    );

    scuba.log();
}

fn log_query_telemetry_impl(
    query_tel: QueryTelemetry,
    sql_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: Vec<RepositoryId>,
    query_name: &str,
) -> Result<()> {
    #[cfg(not(fbcode_build))]
    {
        // To remove typechecker unused variable warning in OSS
        let _ = (sql_tel, granularity);
    }
    match query_tel {
        #[cfg(fbcode_build)]
        QueryTelemetry::MySQL(telemetry) => {
            // Log to scuba
            log_mysql_query_telemetry(telemetry, sql_tel, granularity, repo_ids, query_name)
        }
        _ => Err(anyhow!("Unsupported query telemetry type")),
    }
}

#[cfg(fbcode_build)]
fn log_mysql_query_telemetry(
    query_tel: MysqlQueryTelemetry,
    sql_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: Vec<RepositoryId>,
    query_name: &str,
) -> Result<()> {
    let mut scuba = setup_scuba_sample(sql_tel, granularity, repo_ids, Some(query_name))?;

    scuba.add("success", 1);
    STATS::success.add_value(1);

    let opt_instance_type = query_tel.instance_type().cloned();

    let read_tables = query_tel.read_tables().iter().collect::<Vec<_>>();
    let write_tables = query_tel.write_tables().iter().collect::<Vec<_>>();

    scuba.add("instance_type", opt_instance_type.clone());
    if let Some(instance_type) = opt_instance_type {
        // Success
        STATS::success_instance.add_value(1, (instance_type.clone(),));

        // CPU and Delay RRU by instance type
        if let Some(client_stats) = query_tel.client_stats() {
            STATS::cpu_rru_instance.add_value(
                (1000.0 * client_stats.cpu_rru) as i64,
                (instance_type.clone(),),
            );
            STATS::delay_rru_instance.add_value(
                (1000.0 * client_stats.delay_rru) as i64,
                (instance_type.clone(),),
            );
            STATS::task_full_delay_rru_instance.add_value(
                (1000.0 * client_stats.task_full_delay_rru) as i64,
                (instance_type.clone(),),
            );
            STATS::task_some_delay_rru.add_value(
                (1000.0 * client_stats.task_some_delay_rru) as i64,
                (instance_type.clone(),),
            );
        };

        // Table stats
        read_tables.iter().for_each(|&table| {
            STATS::read_tables.add_value(1, (table.clone(), instance_type.clone()))
        });

        write_tables.iter().for_each(|&table| {
            STATS::write_tables.add_value(1, (table.clone(), instance_type.clone()))
        });
    }

    // CPU and Delay RRU by instance type
    if let Some(client_stats) = query_tel.client_stats() {
        STATS::cpu_rru.add_value((1000.0 * client_stats.cpu_rru) as i64);
        STATS::delay_rru.add_value((1000.0 * client_stats.delay_rru) as i64);
        STATS::full_delay_rru.add_value((1000.0 * client_stats.full_delay_rru) as i64);
        STATS::max_cpu_rru.add_value((1000.0 * client_stats.max_rru) as i64);
    };

    scuba.add("read_tables", read_tables);
    scuba.add("write_tables", write_tables);

    for wait_stats in query_tel.wait_stats() {
        STATS::wait_count.add_value(
            wait_stats.wait_count as i64,
            (wait_stats.wait_type.clone(),),
        );
        wait_stats.wait_time.inspect(|wt| {
            STATS::wait_time.add_value(*wt as i64, (wait_stats.wait_type.clone(),));
        });

        wait_stats.signal_time.inspect(|st| {
            STATS::signal_time.add_value(*st as i64, (wait_stats.wait_type.clone(),));
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

fn log_transaction_telemetry_impl(
    txn_tel: TransactionTelemetry,
    sql_tel: &SqlQueryTelemetry,
) -> Result<()> {
    let mut scuba = setup_scuba_sample(
        sql_tel,
        TelemetryGranularity::Transaction,
        txn_tel.repo_ids.into_iter().collect::<Vec<_>>(),
        None,
    )?;

    scuba.add("success", 1);

    scuba.add(
        "read_tables",
        txn_tel.read_tables.into_iter().collect::<Vec<_>>(),
    );
    scuba.add(
        "write_tables",
        txn_tel.write_tables.into_iter().collect::<Vec<_>>(),
    );

    // Log the Scuba sample for debugging when log-level is set to trace.
    tracing::trace!(
        "Logging transaction telemetry to scuba: {0:#?}",
        scuba.get_sample()
    );

    scuba.log();

    Ok(())
}

/// Sets fields that are present in both successful and failed queries.
fn setup_scuba_sample(
    sql_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    repo_ids: Vec<RepositoryId>,
    query_name: Option<&str>,
) -> Result<MononokeScubaSampleBuilder> {
    let fb = sql_tel.fb().clone();

    // Log to file if SQL_TELEMETRY_SCUBA_FILE_PATH is set (for testing)
    let mut scuba = if let Ok(scuba_file_path) = std::env::var("SQL_TELEMETRY_SCUBA_FILE_PATH") {
        MononokeScubaSampleBuilder::with_discard().with_log_file(scuba_file_path)?
    } else {
        MononokeScubaSampleBuilder::new(fb, SQL_TELEMETRY_SCUBA_TABLE)?
    };

    scuba.add_metadata(sql_tel.metadata());

    scuba.add_common_server_data();

    scuba.add("granularity", format!("{:?}", granularity));
    scuba.add("query_name", query_name);

    let jk_sample_rate =
        justknobs::get_as::<u64>("scm/mononoke:sql_telemetry_sample_rate", None).unwrap_or(10);

    match NonZeroU64::new(jk_sample_rate).ok_or(anyhow!("Sample rate must be a positive number")) {
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
                self.add_mysql_query_telemetry(mysql_tel);
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

    #[cfg(fbcode_build)]
    fn add_mysql_query_telemetry(&mut self, query_tel: MysqlQueryTelemetry) {
        self.read_tables.extend(query_tel.read_tables);
        self.write_tables.extend(query_tel.write_tables);
    }
}

// Documentation of MySQL Client Logs: https://fburl.com/wiki/e21tf16l
define_stats! {
    prefix = "mononoke.sql_telemetry";
    success: timeseries("success"; Sum, Average),
    success_instance: dynamic_timeseries(
        "success.instance.{}", (instance_type: String);
        Sum, Average
    ),
    error: timeseries("error"; Sum, Average),

    // Wait stats
    wait_count: dynamic_timeseries(
        "wait_count.{}", (wait_event_type: String);
        Sum, Average
    ),
    wait_time: dynamic_timeseries(
        "wait_time.{}", (wait_event_type: String);
        Sum, Average
    ),
    signal_time: dynamic_timeseries(
        "signal_time.{}", (wait_event_type: String);
         Sum, Average
    ),

    // CPU and Delay RRU for all tasks
    cpu_rru: timeseries("cpu_milli_rru"; Sum, Average),
    max_cpu_rru: timeseries("max_cpu_milli_rru"; Sum),
    delay_rru: timeseries("delay_milli_rru"; Sum, Average),
    full_delay_rru: timeseries("full_delay_milli_rru";  Sum, Average),

    // CPU and Delay RRU split by instance type (e.g. Primary or Secondary)
    cpu_rru_instance: dynamic_timeseries(
        "cpu_milli_rru.instance.{}", (instance_type: String);
         Sum, Average
    ),
    delay_rru_instance: dynamic_timeseries(
        "delay_milli_rru.instance.{}", (instance_type: String);
         Sum, Average
    ),
    task_full_delay_rru_instance: dynamic_timeseries(
        "task_full_delay_milli_rru.instance.{}", (instance_type: String);
         Sum, Average
    ),
    task_some_delay_rru: dynamic_timeseries(
        "task_some_delay_milli_rru.instance.{}", (instance_type: String);
         Sum, Average
    ),

    // Table stats
    read_tables: dynamic_timeseries(
        "reads.{}.instance.{}", (table: String, instance_type: String);
        Count, Sum
    ),
    write_tables: dynamic_timeseries(
        "writes.{}.instance.{}", (table: String, instance_type: String);
        Count, Sum
    ),

}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use sql::QueryTelemetry;
#[cfg(fbcode_build)]
use sql::mysql::MysqlQueryTelemetry;
use sql_query_telemetry::SqlQueryTelemetry;

const SQL_TELEMETRY_SCUBA_TABLE: &str = "mononoke_sql_telemetry";

#[derive(Clone, Debug, Eq, PartialEq, Copy)]
pub enum TelemetryGranularity {
    /// From a single query
    Query,
    /// From a query within a transaction
    TransactionQuery,
    /// From a transaction (i.e. when committing it)
    Transaction,
}

// TODO(T223577767): make these args required and remove the need for
// `log_query_telemetry_impl`
pub fn log_query_telemetry(
    opt_tel: Option<QueryTelemetry>,
    opt_sql_tel: Option<SqlQueryTelemetry>,
    granularity: TelemetryGranularity,

    opt_repo_id: Option<RepositoryId>,
) -> Result<()> {
    match (opt_tel, opt_sql_tel) {
        (Some(query_tel), Some(sql_tel)) => {
            log_query_telemetry_impl(query_tel, sql_tel, granularity, opt_repo_id)
        }
        // TODO(T223577767): handle case when there's no telemetry
        _ => Ok(()),
    }
}

/// Log query errors to Scuba on a best-effort basis.
pub fn log_query_error(
    opt_tel: &Option<SqlQueryTelemetry>,
    err: &Error,
    granularity: TelemetryGranularity,
) {
    let sql_tel = match opt_tel.as_ref() {
        Some(sql_tel) => sql_tel,
        None => return,
    };

    let mut scuba = match setup_scuba_sample(sql_tel, granularity) {
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

    // Log the Scuba sample for debugging when log-level is set to trace.
    tracing::trace!(
        "Logging query telemetry to scuba: {0:#?}",
        scuba.get_sample()
    );

    scuba.log();
}

fn log_query_telemetry_impl(
    query_tel: QueryTelemetry,
    sql_tel: SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    opt_repo_id: Option<RepositoryId>,
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
            log_mysql_query_telemetry(telemetry, sql_tel, granularity, opt_repo_id)
        }
        _ => Err(anyhow!("Unsupported query telemetry type")),
    }
}

#[cfg(fbcode_build)]
fn log_mysql_query_telemetry(
    query_tel: MysqlQueryTelemetry,
    sql_tel: SqlQueryTelemetry,
    granularity: TelemetryGranularity,
    opt_repo_id: Option<RepositoryId>,
) -> Result<()> {
    let mut scuba = setup_scuba_sample(&sql_tel, granularity)?;

    scuba.add("success", 1);

    scuba.add("instance_type", query_tel.instance_type());
    scuba.add(
        "read_tables",
        query_tel.read_tables().iter().collect::<Vec<_>>(),
    );
    scuba.add(
        "write_tables",
        query_tel.write_tables().iter().collect::<Vec<_>>(),
    );

    if let Some(repo_id) = opt_repo_id {
        scuba.add("repo_id", repo_id.id());
    }

    for wait_stats in query_tel.wait_stats() {
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

/// Sets fields that are present in both successful and failed queries.
fn setup_scuba_sample(
    sql_tel: &SqlQueryTelemetry,
    granularity: TelemetryGranularity,
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

    Ok(scuba)
}

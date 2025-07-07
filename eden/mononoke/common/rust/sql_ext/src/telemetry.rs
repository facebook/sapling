/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use scuba_ext::MononokeScubaSampleBuilder;
use sql::QueryTelemetry;
#[cfg(fbcode_build)]
use sql::mysql::MysqlQueryTelemetry;
use sql_query_telemetry::SqlQueryTelemetry;

const SQL_TELEMETRY_SCUBA_TABLE: &str = "mononoke_sql_telemetry";

#[derive(Clone, Debug, Eq, PartialEq)]
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
) -> Result<()> {
    match (opt_tel, opt_sql_tel) {
        (Some(query_tel), Some(sql_tel)) => {
            log_query_telemetry_impl(query_tel, sql_tel, granularity)
        }
        // TODO(T223577767): handle case when there's no telemetry
        _ => Ok(()),
    }
}

fn log_query_telemetry_impl(
    query_tel: QueryTelemetry,
    sql_tel: SqlQueryTelemetry,
    granularity: TelemetryGranularity,
) -> Result<()> {
    match query_tel {
        #[cfg(fbcode_build)]
        QueryTelemetry::MySQL(telemetry) => {
            // Log to scuba
            log_mysql_query_telemetry(telemetry, sql_tel, granularity)
        }
        _ => Err(anyhow!("Unsupported query telemetry type")),
    }
}

#[cfg(fbcode_build)]
fn log_mysql_query_telemetry(
    query_tel: MysqlQueryTelemetry,
    sql_tel: SqlQueryTelemetry,
    granularity: TelemetryGranularity,
) -> Result<()> {
    let fb = sql_tel.fb().clone();

    // Log to file if SQL_TELEMETRY_SCUBA_FILE_PATH is set (for testing)
    let mut scuba = if let Ok(scuba_file_path) = std::env::var("SQL_TELEMETRY_SCUBA_FILE_PATH") {
        MononokeScubaSampleBuilder::with_discard().with_log_file(scuba_file_path)?
    } else {
        MononokeScubaSampleBuilder::new(fb, SQL_TELEMETRY_SCUBA_TABLE)?
    };

    if let Some(cri) = sql_tel.client_request_info() {
        scuba.add_client_request_info(cri);
    };

    scuba.add("granularity", format!("{:?}", granularity));
    scuba.add("instance_type", query_tel.instance_type());
    scuba.add(
        "read_tables",
        query_tel.read_tables().iter().collect::<Vec<_>>(),
    );
    scuba.add(
        "write_tables",
        query_tel.write_tables().iter().collect::<Vec<_>>(),
    );

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

    scuba.log();

    Ok(())
}

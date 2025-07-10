/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use clientinfo::ClientRequestInfo;
use fbinit::FacebookInit;
use metadata::Metadata;
use mononoke_macros::mononoke;
use sql_query_config::SqlQueryConfig;
use sql_query_telemetry::SqlQueryTelemetry;

use crate::mononoke_queries;

mononoke_queries! {
    read TestQuery(param_str: String, param_uint: u64) -> (u64, Option<i32>, String, i64) {
        "SELECT 44, NULL, {param_str}, {param_uint}"
    }
    pub(crate) cacheable read TestQuery2() -> (u64, Option<String>) {
        "SELECT 44, NULL"
    }
    pub(super) write TestQuery3(values: (
        val1: i32,
    )) {
        none,
        "INSERT INTO my_table (num, str) VALUES {values}"
    }
    write TestQuery4(id: &str) {
        none,
        mysql("DELETE FROM my_table where id = {id}")
        sqlite("DELETE FROM mytable2 where id = {id}")
    }

    read ReadQuery1(id: u64) -> (i64) {
        "SELECT x FROM mononoke_queries_test WHERE ID > {id} LIMIT 10"
    }

    write WriteQuery1(values: (x: i64)) {
        none,
        "INSERT INTO mononoke_queries_test (x) VALUES {values}"
    }
}

#[cfg(fbcode_build)]
#[cfg(test)]
mod facebook {
    use std::collections::HashMap;
    use std::collections::HashSet;

    use maplit::hashmap;
    use maplit::hashset;
    use sql_tests_lib::mysql_test_lib::setup_mysql_test_connection;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_basic_scuba_logging(fb: FacebookInit) -> anyhow::Result<()> {
        // Set log file in SQL_TELEMETRY_SCUBA_FILE_PATH environment variable
        let temp_file = tempfile::NamedTempFile::new()?;
        let temp_path = temp_file.path().to_str().unwrap().to_string();
        unsafe {
            std::env::set_var("SQL_TELEMETRY_SCUBA_FILE_PATH", &temp_path);
        }

        let connection: sql::Connection = setup_mysql_test_connection(
            fb,
            "CREATE TABLE IF NOT EXISTS mononoke_queries_test(
             x INT,
             y DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
             test CHAR(64),
             id INT AUTO_INCREMENT,
             PRIMARY KEY(id)
         )",
        )
        .await?;
        let client_info = ClientInfo::new_with_entry_point(ClientEntryPoint::Tests)?;
        let cri = client_info
            .request_info
            .clone()
            .expect("client request info missing");

        println!("cri: {:#?}", cri);

        let mut metadata = Metadata::default();
        metadata.add_client_info(client_info);

        let tel_logger = SqlQueryTelemetry::new(fb, metadata);

        let _res = WriteQuery1::query(&connection, Some(tel_logger.clone()), &[(&1i64,), (&2i64,)])
            .await?;

        let _res = ReadQuery1::query(&connection, Some(tel_logger), &1).await?;

        // Values that we expect to always be the same in all the samples.
        let consistent_values: HashMap<String, String> = hashmap! {
            "client_correlator".to_string() => cri.correlator.to_string(),
            "client_entry_point".to_string() => ClientEntryPoint::Tests.to_string(),
        };

        // Columns expected to be logged in all samples.
        let expected_in_all: HashSet<String> = hashset! {
          "avg_rru",
          "build_revision",
          "build_rule",
          "client_correlator",
          "client_entry_point",
          "client_identities",
          "client_main_id",
          "cpu_rru",
          "datacenter",
          "delay_rru",
          "delay_rru",
          "full_delay_rru",
          "granularity",
          "instance_type",
          "max_rru",
          "min_rru",
          "overlimit_delay_rru",
          "region_datacenter_prefix",
          "region",
          "server_hostname",
          "session_uuid",
          "some_delay_rru",
          "success",
          "task_full_delay_rru",
          "task_some_delay_rru",
          "time",
        }
        .into_iter()
        .map(String::from)
        .collect();

        // Columns expected in some samples, but not necessarily all.
        let expected_in_some: HashSet<String> = hashset! {
            "read_tables",
            "signal_time_ENQUEUE",
            "wait_count_ENQUEUE",
            "wait_time_ENQUEUE",
            "write_tables",
        }
        .into_iter()
        .map(String::from)
        .collect();

        // Read the temp file and print its content
        let content = std::fs::read_to_string(&temp_path)?;

        // Uncomment to debug the entire log file
        // println!("Scuba log content: {:#?}", content);

        // Extract and print all columns from the scuba logs
        let columns = extract_all_scuba_columns(&content, expected_in_all, consistent_values);

        // For debugging purposes. By default will only print if test fails.
        println!("All columns logged in scuba samples: {:#?}", columns);

        assert!(
            expected_in_some.is_subset(&columns),
            "Expected columns that should be in at least one sample are missing"
        );

        Ok(())
    }

    // TODO(T223577767): test transaction-level metadata, e.g. run multiple queries
    // for different repos and ensure they are all logged together.

    /// Extracts all column names from scuba samples in the log content
    fn extract_all_scuba_columns(
        log_content: &str,
        expected_in_all: HashSet<String>,
        consistent_values: HashMap<String, String>,
    ) -> HashSet<String> {
        log_content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .fold(HashSet::new(), |mut all_columns, json| {
            // println!("json: {:#?}", json);
            let sample_columns = extract_columns_from_sample(&json, &consistent_values);

            println!("sample_columns: {:#?}", sample_columns);
            assert!(
                expected_in_all.is_subset(&sample_columns),
                "Expected columns that should be in all samples are missing: {0:#?}. Sample: {1:#?}",
                expected_in_all
                    .difference(&sample_columns)
                    .collect::<Vec<_>>(),
                    log_content
            );

            all_columns.extend(sample_columns);
            all_columns
        })
    }

    /// Extracts column names from a single scuba sample
    fn extract_columns_from_sample(
        sample: &serde_json::Value,
        consistent_values: &HashMap<String, String>,
    ) -> HashSet<String> {
        // Check each category (normal, int, double, normvector)
        if let Some(obj) = sample.as_object() {
            return obj
                .iter()
                .fold(HashSet::new(), |mut acc, (_category, value)| {
                    if let Some(category_obj) = value.as_object() {
                        println!("category_obj: {:#?}", category_obj);
                        consistent_values.iter().for_each(|(exp_key, exp_v)| {
                            // Check if the key is inside the value object
                            // and if it is, assert the value is the same as expected
                            if let Some(value) = category_obj.get(exp_key) {
                                assert_eq!(
                                    exp_v,
                                    value,
                                    "Expected value {0} for key {1} but got {2}",
                                    exp_v,
                                    exp_key,
                                    value.as_str().unwrap_or_default()
                                );
                            };
                        });
                        // Add each column name from this category
                        acc.extend(category_obj.keys().cloned());
                    }
                    acc
                });
        }

        HashSet::new()
    }
}

#[allow(
    dead_code,
    unreachable_code,
    unused_variables,
    clippy::diverging_sub_expression,
    clippy::todo
)]
#[ignore]
#[mononoke::fbinit_test]
async fn should_compile(fb: FacebookInit) -> anyhow::Result<()> {
    let config: &SqlQueryConfig = todo!();
    let connection: &sql::Connection = todo!();

    let cri = ClientRequestInfo::new(ClientEntryPoint::Sapling);
    let client_info = ClientInfo::new()?;
    let mut metadata = Metadata::default();
    metadata.add_client_info(client_info);

    let tel_logger = SqlQueryTelemetry::new(fb, metadata);
    TestQuery::query(connection, None, todo!(), todo!()).await?;
    TestQuery::query_with_transaction(todo!(), None, todo!(), todo!()).await?;
    TestQuery2::query(config, None, connection, None::<SqlQueryTelemetry>).await?;
    TestQuery2::query(
        config,
        Some(std::time::Duration::from_secs(60)),
        connection,
        None,
    )
    .await?;
    TestQuery2::query_with_transaction(todo!(), None).await?;
    TestQuery3::query(connection, None, &[(&12,)]).await?;
    TestQuery3::query_with_transaction(todo!(), None, &[(&12,)]).await?;
    TestQuery4::query(connection, None, &"hello").await?;
    TestQuery::query(connection, Some(tel_logger), todo!(), todo!()).await?;
    TestQuery2::query(config, None, connection, Some(tel_logger)).await?;
    TestQuery3::query(connection, Some(tel_logger), &[(&12,)]).await?;
    TestQuery4::query(connection, Some(tel_logger), &"hello").await?;
    TestQuery::query(connection, None, todo!(), todo!()).await?;
    TestQuery2::query(config, None, connection, None).await?;
    TestQuery3::query(connection, None, &[(&12,)]).await?;
    TestQuery4::query(connection, None, &"hello").await?;
    Ok(())
}

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

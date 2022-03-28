/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for controlling MySql
// Defaults are derived from `sql_ext::facebook::mysql`
// https://fburl.com/diffusion/n5isd68j, last synced on 17/12/2020
#[derive(Args, Debug)]
pub struct MysqlArgs {
    /// Connect to MySql master only.
    #[clap(long)]
    pub mysql_master_only: bool,

    /// Size of the MySql connection pool
    #[clap(long, default_value = "10000")]
    pub mysql_pool_limit: usize,

    /// MySql connection pool per key limit
    #[clap(long, default_value = "100")]
    pub mysql_pool_per_key_limit: u64,

    /// Number of threads in MySql connection pool (number of real pools)
    #[clap(long, default_value = "10")]
    pub mysql_pool_threads_num: i32,

    /// Mysql connection pool age timeout in millisecs
    #[clap(long, default_value = "60000")]
    pub mysql_pool_age_timeout: u64,

    /// Mysql connection pool idle timeout in millisecs
    #[clap(long, default_value = "4000")]
    pub mysql_pool_idle_timeout: u64,

    /// Size of the MySql connection pool for SqlBlob
    #[clap(long, default_value = "10000", alias = "mysql-sqblob-pool-limit")]
    pub mysql_sqlblob_pool_limit: usize,

    /// MySql connection pool per key limit for SqlBlob
    #[clap(long, default_value = "100", alias = "mysql-sqblob-pool-per-key-limit")]
    pub mysql_sqlblob_pool_per_key_limit: u64,

    /// Number of threads in MySql connection pool (number of real pools) for
    /// SqlBlob
    #[clap(long, default_value = "10", alias = "mysql-sqblob-pool-threads-num")]
    pub mysql_sqlblob_pool_threads_num: i32,

    /// MySql connection pool age timeout in millisecs for SqlBlob
    #[clap(long, default_value = "60000", alias = "mysql-sqblob-pool-age-timeout")]
    pub mysql_sqlblob_pool_age_timeout: u64,

    /// MySql connection pool idle timeout in millisecs for SqlBlob
    #[clap(long, default_value = "4000", alias = "mysql-sqblob-pool-idle-timeout")]
    pub mysql_sqlblob_pool_idle_timeout: u64,

    /// MySql connection open timeout in millisecs
    #[clap(long, default_value = "3000")]
    pub mysql_conn_open_timeout: u64,

    /// Mysql query time limit in millisecs
    #[clap(long, default_value = "10000", alias = "mysql-max-query-time")]
    pub mysql_query_time_limit: u64,
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use caching_ext::*;
use futures_retry::retry;
use itertools::Itertools;
use maplit::hashmap;
use maplit::hashset;
use memcache::KeyGen;
#[cfg(fbcode_build)]
use mysql_client::MysqlError;
use sql_query_config::CachingConfig;

const RETRY_ATTEMPTS: usize = 2;

// This wraps around rust/shed/sql::queries, check that macro: https://fburl.com/code/semq9xm3
/// Define SQL queries that automatically retry on certain errors.
///
/// Caching can be enabled on a read query by:
/// - Adding "cacheable" keyword to your query.
/// - Make sure all parameters (input) to the query implement the Hash trait.
/// - Making sure the return values (output) implement Serialize, Deserialize, and
///   bincode::{Encode, Decode}.
///
/// Queries that return no rows are not cached to allow later retries to succeed.
#[macro_export]
macro_rules! mononoke_queries {
    () => {};

    // Read query with a single expression. Redirect to read query with same expression for mysql and sqlite.
    (
        $vi:vis read $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) -> ($( $rtype:ty ),* $(,)*) { $q:expr }
        $( $rest:tt )*
    ) => {
        $crate::mononoke_queries! {
            $vi read $name (
                $( $pname: $ptype, )*
                $( >list $lname: $ltype )*
            ) -> ($( $rtype ),*) { mysql($q) sqlite($q) }
            $( $rest )*
        }
    };
    // Read query with a single expression and cache. Redirect to read query with same expression for mysql and sqlite.
    (
        $vi:vis cacheable read $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) -> ($( $rtype:ty ),* $(,)*) { $q:expr }
        $( $rest:tt )*
    ) => {
        $crate::mononoke_queries! {
            $vi cacheable read $name (
                $( $pname: $ptype, )*
                $( >list $lname: $ltype )*
            ) -> ($( $rtype ),*) { mysql($q) sqlite($q) }
            $( $rest )*
        }
    };

    // Full read query without cache. Call `sql::queries!` and re-export stuff, wrapped in retries, on a new module.
    (
        $vi:vis read $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) -> ($( $rtype:ty ),* $(,)*) { mysql($mysql_q:expr) sqlite($sqlite_q:expr) }
        $( $rest:tt )*
    ) => {
        $crate::_macro_internal::paste::item! {
            $crate::_macro_internal::queries! {
                pub read [<$name Impl>] (
                    $( $pname: $ptype, )*
                    $( >list $lname: $ltype )*
                ) -> ($( $rtype ),*) { mysql($mysql_q) sqlite($sqlite_q) }
            }

            #[allow(non_snake_case)]
            $vi mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[allow(unused_imports)]
                use $crate::_macro_internal::*;

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    sql_query_tel: SqlQueryTelemetry,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    let query_name = stringify!($name);
                    let shard_name = connection.shard_name();
                    query_with_retry_no_cache(
                        || {
                            borrowed!(sql_query_tel);
                            async move {
                                let cri = sql_query_tel.client_request_info();
                                // Convert ClientRequestInfo to string if present
                                let cri_str = cri.map(|cri| serde_json::to_string(cri)).transpose()?;

                                let granularity = TelemetryGranularity::Query;

                                // Check if any parameter is a RepositoryId and pass it to telemetry
                                let repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);

                                let (res, opt_tel) = [<$name Impl>]::commented_query(
                                    connection.sql_connection(),
                                    cri_str.as_deref(),
                                    $( $pname, )*
                                    $( $lname, )*
                                ).await.inspect_err(|e| {
                                    log_query_error(
                                        &sql_query_tel,
                                        &e,
                                        granularity,
                                        repo_ids.clone(),
                                        query_name,
                                        shard_name.as_ref(),
                                    )
                                })?;

                                log_query_telemetry(
                                    opt_tel,
                                    &sql_query_tel,
                                    granularity,
                                    repo_ids,
                                    query_name,
                                    shard_name.as_ref(),
                                )?;


                                Ok(res)
                            }
                        },
                        shard_name,
                        query_name,
                    ).await
                }

                #[allow(dead_code)]
                pub async fn query_with_transaction(
                    transaction: Transaction,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    let query_name = stringify!($name);
                    let query_repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);
                    $crate::read_query_with_transaction!(
                        $name,
                        transaction,
                        query_repo_ids,
                        query_name,
                        ($( $pname: $ptype ),*),
                        ($( $lname: $ltype )*)
                    )
                }

            }

            $crate::mononoke_queries! { $( $rest )* }
        }
    };

    // Full read query. Call `sql::queries!` and re-export stuff, wrapped in retries, on a new module.
    (
        $vi:vis cacheable read $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) -> ($( $rtype:ty ),* $(,)*) { mysql($mysql_q:expr) sqlite($sqlite_q:expr) }
        $( $rest:tt )*
    ) => {
        $crate::_macro_internal::paste::item! {
            $crate::_macro_internal::queries! {
                pub read [<$name Impl>] (
                    $( $pname: $ptype, )*
                    $( >list $lname: $ltype )*
                ) -> ($( $rtype ),*) { mysql($mysql_q) sqlite($sqlite_q) }
            }

            #[allow(non_snake_case)]
            $vi mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[allow(unused_imports)]
                use $crate::_macro_internal::*;

                #[allow(dead_code)]
                pub async fn query(
                    config: &SqlQueryConfig,
                    cache_ttl: Option<std::time::Duration>,
                    connection: &Connection,
                    sql_query_tel: SqlQueryTelemetry,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    // Prepare cache data
                    let mut hasher = Hash128::with_seed(0);
                    $(
                        $pname.hash(&mut hasher);
                    )*
                    $(
                        $lname.hash(&mut hasher);
                    )*
                    stringify!($name).hash(&mut hasher);
                    stringify!($mysql_q).hash(&mut hasher);
                    stringify!($sqlite_q).hash(&mut hasher);
                    let key = hasher.finish_ext();
                    let data = CacheData {key, config: config.caching.as_ref(), cache_ttl };

                    let query_name = stringify!($name);
                    let shard_name = connection.shard_name();
                    // Execute query with caching
                    let res = query_with_retry(
                        data,
                        || {
                            borrowed!(sql_query_tel);
                            let cri = sql_query_tel.client_request_info();
                            async move {
                                // Convert ClientRequestInfo to string if present
                                let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;

                                let granularity = TelemetryGranularity::Query;

                                // Check if any parameter is a RepositoryId and pass it to telemetry
                                let repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);

                                let (res, opt_tel) = [<$name Impl>]::commented_query(
                                    connection.sql_connection(),
                                    cri_str.as_deref(),
                                    $( $pname, )*
                                    $( $lname, )*

                                ).await.inspect_err(|e| {
                                    log_query_error(
                                        &sql_query_tel,
                                        &e,
                                        granularity,
                                        repo_ids.clone(),
                                        &query_name,
                                        shard_name.as_ref(),

                                    )
                                })?;

                                log_query_telemetry(
                                    opt_tel,
                                    &sql_query_tel,
                                    granularity,
                                    repo_ids,
                                    query_name,
                                    shard_name.as_ref(),
                                )?;
                                Ok(CachedQueryResult(res))
                            }
                        },
                        shard_name,
                        query_name,
                    ).await?.0;

                    Ok(res)
                }

                #[allow(dead_code)]
                pub async fn query_with_transaction(
                    transaction: Transaction,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    let query_name = stringify!($name);
                    let query_repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);
                    $crate::read_query_with_transaction!(
                        $name,
                        transaction,
                        query_repo_ids,
                        query_name,
                        ($( $pname: $ptype ),*),
                        ($( $lname: $ltype )*)
                    )
                }

            }

            $crate::mononoke_queries! { $( $rest )* }
        }
    };

    // Write query with a single expression. Redirect to write query with same expression for mysql and sqlite.
    (
        $vi:vis write $name:ident (
            values: ($( $vname:ident: $vtype:ty ),* $(,)*)
            $( , $pname:ident: $ptype:ty )* $(,)*
        ) { $qtype:ident, $q:expr }
        $( $rest:tt )*
    ) => {
        $crate::mononoke_queries! {
            $vi write $name (
                values: ( $( $vname: $vtype ),* )
                $( , $pname: $ptype )*
            ) { $qtype, mysql($q) sqlite($q) }
            $( $rest )*
        }
    };

    // Full write query with a list of values. Call `sql::queries!` and re-export stuff, wrapped in retries, on a new module.
    (
        $vi:vis write $name:ident (
            values: ($( $vname:ident: $vtype:ty ),* $(,)*)
            $( , $pname:ident: $ptype:ty )* $(,)*
        ) { $qtype:ident, mysql($mysql_q:expr) sqlite($sqlite_q:expr) }
        $( $rest:tt )*
    ) => {
        $crate::_macro_internal::paste::item! {
            $crate::_macro_internal::queries! {
                pub write [<$name Impl>] (
                    values: ( $( $vname: $vtype ),* )
                    $( , $pname: $ptype )*
                ) { $qtype, mysql($mysql_q) sqlite($sqlite_q) }
            }

            #[allow(non_snake_case)]
            $vi mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[allow(unused_imports)]
                use $crate::_macro_internal::*;

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    sql_query_tel: SqlQueryTelemetry,
                    values: &[($( & $vtype, )*)],
                    $( $pname: &$ptype ),*
                ) -> Result<WriteResult> {
                    let query_name = stringify!($name);
                    let cri = sql_query_tel.client_request_info();
                    // Convert ClientRequestInfo to string if present
                    let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;
                    let shard_name = connection.shard_name();

                    let granularity = TelemetryGranularity::Query;

                    // Extract repo IDs from values parameter
                    let values_repo_ids: Vec<RepositoryId> =
                        $crate::_macro_internal::extract_repo_ids_from_values!(($($vtype,)*));
                    // Check if any parameter is a RepositoryId and pass it to telemetry
                    let repo_ids: Vec<RepositoryId> =
                        $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*)
                        .into_iter()
                        .chain(values_repo_ids)
                        .collect();


                    let write_res = query_with_retry_no_cache(
                        || [<$name Impl>]::commented_query(
                            connection.sql_connection(),
                            cri_str.as_deref(),
                            values
                            $( , $pname )*
                        ),
                        shard_name,
                        query_name,
                    ).await.inspect_err(|e| {
                        log_query_error(
                            &sql_query_tel,
                            &e,
                            granularity,
                            repo_ids.clone(),
                            &query_name,
                            shard_name.as_ref(),
                        )
                    })?;

                    let opt_tel = write_res.query_telemetry().clone();

                    log_query_telemetry(
                        opt_tel,
                        &sql_query_tel,
                        granularity,
                        repo_ids,
                        &query_name,
                        shard_name.as_ref(),
                    )?;

                    Ok(write_res)

                }

                #[allow(dead_code)]
                pub async fn query_with_transaction(
                    transaction: Transaction,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<(Transaction, WriteResult)> {
                    let query_name = stringify!($name);

                    let Transaction {
                        inner: sql_txn,
                        txn_telemetry,
                        sql_query_tel,
                        shard_name,
                    } = transaction;

                    let cri = sql_query_tel.client_request_info();
                    // Convert ClientRequestInfo to string if present
                    let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;

                    let granularity = TelemetryGranularity::TransactionQuery;

                    // Extract repo IDs from values parameter
                    let values_repo_ids: Vec<RepositoryId> =
                        $crate::_macro_internal::extract_repo_ids_from_values!(($($vtype,)*));
                    // Check if any parameter is a RepositoryId and pass it to telemetry
                    let query_repo_ids: Vec<RepositoryId> =
                        $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*)
                        .into_iter()
                        .chain(values_repo_ids)
                        .collect();


                    let (sql_txn, write_res) = [<$name Impl>]::commented_query_with_transaction(
                        sql_txn,
                        cri_str.as_deref(),
                        values
                        $( , $pname )*
                    ).await.inspect_err(|e| {
                        log_query_error(
                            &sql_query_tel,
                            &e,
                            granularity,
                            query_repo_ids.clone(),
                            &query_name,
                            shard_name.as_ref(),
                        )
                    })?;

                    let opt_tel = write_res.query_telemetry().clone();

                   let txn = Transaction::from_transaction_query_result(
                        sql_txn,
                        opt_tel,
                        txn_telemetry,
                        sql_query_tel,
                        query_repo_ids,
                        granularity,
                        query_name,
                        shard_name,
                    )?;

                    Ok((txn, write_res))

                }
            }

            $crate::mononoke_queries! { $( $rest )* }
        }
    };

    // Write query with a single expression. Redirect to write query with same expression for mysql and sqlite.
    (
        $vi:vis write $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) { $qtype:ident, $q:expr }
        $( $rest:tt )*
    ) => {
        $crate::mononoke_queries! {
            $vi write $name (
                $( $pname: $ptype, )*
                $( >list $lname: $ltype )*
            ) { $qtype, mysql($q) sqlite($q) }
            $( $rest )*
        }
    };

    // Full write query without a list of values. Call `sql::queries!` and re-export stuff, wrapped in retries, on a new module.
    (
        $vi:vis write $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) { $qtype:ident, mysql($mysql_q:expr) sqlite($sqlite_q:expr) }
        $( $rest:tt )*
    ) => {
        $crate::_macro_internal::paste::item! {
            $crate::_macro_internal::queries! {
                pub write [<$name Impl>] (
                    $( $pname: $ptype, )*
                    $( >list $lname: $ltype )*
                ) { $qtype, mysql($mysql_q) sqlite($sqlite_q) }
            }

            #[allow(non_snake_case)]
            $vi mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[allow(unused_imports)]
                use $crate::_macro_internal::*;

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    sql_query_tel: SqlQueryTelemetry,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<WriteResult> {
                    let query_name = stringify!($name);
                    let cri = sql_query_tel.client_request_info();
                    // Convert ClientRequestInfo to string if present
                    let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;
                    let shard_name = connection.shard_name();

                    let granularity = TelemetryGranularity::Query;

                    // Check if any parameter is a RepositoryId and pass it to telemetry
                    let repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);

                    let write_res = query_with_retry_no_cache(
                        || [<$name Impl>]::commented_query(
                            connection.sql_connection(),
                            cri_str.as_deref(),
                            $( $pname, )*
                            $( $lname, )*
                        ),
                        shard_name,
                        query_name,
                    ).await.inspect_err(|e| {
                        log_query_error(
                            &sql_query_tel,
                            &e,
                            granularity,
                            repo_ids.clone(),
                            &query_name,
                            shard_name.as_ref(),
                        )
                    })?;

                    let opt_tel = write_res.query_telemetry().clone();

                    log_query_telemetry(
                        opt_tel,
                        &sql_query_tel,
                        granularity,
                        repo_ids,
                        &query_name,
                        shard_name.as_ref(),
                    )?;

                    Ok(write_res)
                }

                // TODO(T223577767): extract duplication from
                // `query_with_transaction` from write queries with values
                #[allow(dead_code)]
                pub async fn query_with_transaction(
                    transaction: Transaction,
                    $( $pname: &$ptype, )*
                    $( $lname: &[ $ltype ], )*
                ) -> Result<(Transaction, WriteResult)> {
                    let query_name = stringify!($name);

                    let Transaction {
                        inner: sql_txn,
                        txn_telemetry,
                        sql_query_tel,
                        shard_name,
                    } = transaction;

                    let cri = sql_query_tel.client_request_info();
                    // Convert ClientRequestInfo to string if present
                    let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;

                    let granularity = TelemetryGranularity::TransactionQuery;

                    // Check if any parameter is a RepositoryId and pass it to telemetry
                    let query_repo_ids = $crate::extract_repo_ids_from_queries!($($pname: $ptype; )*);


                    let (sql_txn, write_res) = [<$name Impl>]::commented_query_with_transaction(
                        sql_txn,
                        cri_str.as_deref()
                        $( , $pname )*
                        $( , $lname )*
                    ).await.inspect_err(|e| {
                        log_query_error(
                            &sql_query_tel,
                            &e,
                            granularity,
                            query_repo_ids.clone(),
                            &query_name,
                            shard_name.as_ref(),
                        )
                    })?;

                    let opt_tel = write_res.query_telemetry().clone();

                    let txn = Transaction::from_transaction_query_result(
                        sql_txn,
                        opt_tel,
                        txn_telemetry,
                        sql_query_tel,
                        query_repo_ids,
                        granularity,
                        &query_name,
                        shard_name,
                    )?;

                    Ok((txn, write_res))
                }
            }

            $crate::mononoke_queries! { $( $rest )* }
        }
    };

}

// Helper macro to generate the body of query_with_transaction for read queries
#[macro_export]
macro_rules! read_query_with_transaction {
    (
        $name:ident,
        $transaction:ident,
        $query_repo_ids:ident,
        $query_name:ident,
        ($( $pname:ident: $ptype:ty ),*),
        ($( $lname:ident: $ltype:ty )*)
    ) => {{
        let granularity = TelemetryGranularity::TransactionQuery;

       let Transaction {
            inner: sql_txn,
            txn_telemetry,
            sql_query_tel,
            shard_name,
        } = $transaction;

        let cri = sql_query_tel.client_request_info();
        let cri_str = cri.map(|cri| serde_json::to_string(&cri)).transpose()?;

        let (sql_txn, (res, opt_tel)) = paste::expr! {
            [<$name Impl>]::commented_query_with_transaction(
                sql_txn,
                cri_str.as_deref(),
                $( $pname, )*
                $( $lname, )*
            )
        }.await.inspect_err(|e| {
            log_query_error(
                &sql_query_tel,
                &e,
                granularity,
                $query_repo_ids.clone(),
                &$query_name,
                shard_name.as_ref(),
            )
        })?;

        let txn = Transaction::from_transaction_query_result(
            sql_txn,
            opt_tel,
            txn_telemetry,
            sql_query_tel,
            $query_repo_ids,
            granularity,
            $query_name,
            shard_name,
        )?;

        Ok((txn, res))
    }};
}

// Helper macro to extract RepositoryId from query parameters
#[macro_export]
macro_rules! extract_repo_ids_from_queries {
    // Base case: no parameters
    () => {{
        Vec::<RepositoryId>::new()
    }};

    // Match RepositoryId with additional parameters
    ($pname:ident: RepositoryId; $($rest:tt)*) => {{
        vec![*$pname]
            .into_iter()
            .chain(
                $crate::extract_repo_ids_from_queries!($($rest)*)
            )
            .collect::<Vec<_>>()
    }};

    // Skip non-RepositoryId parameter and continue with the rest
    ($pname:ident: $ptype:ty; $($rest:tt)*) => {
        $crate::extract_repo_ids_from_queries!($($rest)*)
    };
}

#[cfg(fbcode_build)]
/// See https://fburl.com/sv/uk8w71td for error descriptions
fn retryable_mysql_errno(errno: u32) -> bool {
    match errno {
        // Deadlock error, advice is restarting transaction, so it's retryable
        1213 => true,
        // Admission control errors
        // Safe to retry on writes as well as the query didn't even start
        1914..=1916 => true,
        _ => false,
    }
}

#[cfg(fbcode_build)]
/// Classifies the errors returned by MySQL as retryable or not
/// useful for retry logic that has to cover greater span than single query
/// like retrying the whole transaction.
pub fn should_retry_mysql_query(err: &anyhow::Error) -> bool {
    use MysqlError::*;
    use mysql_client::MysqlError;
    match err.downcast_ref::<MysqlError>() {
        Some(ConnectionOperationError { mysql_errno, .. })
        | Some(QueryResultError { mysql_errno, .. }) => retryable_mysql_errno(*mysql_errno),
        _ => false,
    }
}

#[cfg(not(fbcode_build))]
pub fn should_retry_mysql_query(_err: &anyhow::Error) -> bool {
    false
}

type Key = u128;

pub struct CacheData<'a> {
    pub key: Key,
    pub config: Option<&'a CachingConfig>,
    pub cache_ttl: Option<Duration>,
}

struct QueryCacheStore<'a, F, T> {
    key: Key,
    cache_config: &'a CachingConfig,
    cachelib: CachelibHandler<CachedQueryResult<Vec<T>>>,
    memcache: MemcacheHandler,
    fetcher: F,
    cache_ttl: Option<Duration>,
}

impl<F, T> EntityStore<CachedQueryResult<Vec<T>>> for QueryCacheStore<'_, F, T> {
    fn cachelib(&self) -> &CachelibHandler<CachedQueryResult<Vec<T>>> {
        &self.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        &self.cache_config.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        &self.memcache
    }

    fn cache_determinator(&self, v: &CachedQueryResult<Vec<T>>) -> CacheDisposition {
        if v.0.is_empty() {
            CacheDisposition::Ignore
        } else {
            CacheDisposition::Cache(self.cache_ttl.map_or(CacheTtl::NoTtl, CacheTtl::Ttl))
        }
    }

    caching_ext::impl_singleton_stats!("sql");
}

#[async_trait]
impl<T, F, Fut> KeyedEntityStore<Key, CachedQueryResult<Vec<T>>> for QueryCacheStore<'_, F, T>
where
    T: Send + 'static,
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Result<CachedQueryResult<Vec<T>>>> + Send,
{
    fn get_cache_key(&self, key: &Key) -> String {
        // We just need a unique representation of the key as a String.
        // Let's use base64 as it's smaller than just .to_string()
        base64::engine::general_purpose::STANDARD.encode(key.to_ne_bytes())
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Key>,
    ) -> Result<HashMap<Key, CachedQueryResult<Vec<T>>>> {
        let key = keys.into_iter().exactly_one()?;
        anyhow::ensure!(key == self.key, "Fetched invalid key {}", key);
        let val = (self.fetcher)().await?;
        Ok(hashmap! { key => val })
    }
}

#[derive(Clone)]
#[derive(bincode::Encode, bincode::Decode)]
pub struct CachedQueryResult<T>(pub T);

impl<T> MemcacheEntity for CachedQueryResult<Vec<T>>
where
    T: serde::Serialize + for<'a> serde::Deserialize<'a>,
{
    fn serialize(&self) -> Bytes {
        serde_cbor::to_vec(&self.0)
            .expect("Should serialize cleanly")
            .into()
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        match serde_cbor::from_slice(bytes.as_ref()) {
            Ok(ok) => Ok(Self(ok)),
            Err(_) => Err(McErrorKind::Deserialization),
        }
    }
}

pub async fn query_with_retry_no_cache<T, Fut>(
    do_query: impl Fn() -> Fut + Send + Sync,
    shard_name: &str,
    query_name: &str,
) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>>,
{
    if let Ok(true) = justknobs::eval("scm/mononoke:sql_disable_auto_retries", None, None) {
        return do_query().await;
    }
    Ok(retry(|_| do_query(), Duration::from_secs(10))
        .exponential_backoff(1.2)
        .jitter(Duration::from_secs(5))
        .max_attempts(RETRY_ATTEMPTS)
        .inspect_err(|attempt, e| {
            #[cfg(fbcode_build)]
            if let Some(e) = e.downcast_ref::<MysqlError>() {
                use stats::prelude::DynamicTimeseries;

                use crate::telemetry::STATS;

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
            };
            #[cfg(not(fbcode_build))]
            let _ = (attempt, shard_name, query_name, e);
        })
        .await?
        .0)
}

pub async fn query_with_retry<T, Fut>(
    cache_data: CacheData<'_>,
    do_query: impl Fn() -> Fut + Send + Sync,
    shard_name: &str,
    query_name: &str,
) -> Result<CachedQueryResult<Vec<T>>>
where
    T: Send + bincode::Encode + bincode::Decode<()> + Clone + 'static,
    CachedQueryResult<Vec<T>>: MemcacheEntity,
    Fut: Future<Output = Result<CachedQueryResult<Vec<T>>>> + Send,
{
    if let Ok(true) = justknobs::eval("scm/mononoke:sql_disable_auto_cache", None, None) {
        return query_with_retry_no_cache(&do_query, shard_name, query_name).await;
    }
    let fetch = || query_with_retry_no_cache(&do_query, shard_name, query_name);
    let key = cache_data.key;
    if let Some(config) = cache_data.config.as_ref() {
        let store = QueryCacheStore {
            key: cache_data.key,
            cachelib: config.cache_handler_factory.cachelib(),
            memcache: config.cache_handler_factory.memcache(),
            cache_config: config,
            fetcher: fetch,
            cache_ttl: cache_data.cache_ttl,
        };
        Ok(get_or_fill(&store, hashset! {key})
            .await?
            .into_iter()
            .exactly_one()
            .map_err(|_| anyhow!("Multiple values for a single key"))?
            .1)
    } else {
        fetch().await
    }
}

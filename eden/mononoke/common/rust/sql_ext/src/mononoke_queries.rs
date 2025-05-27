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

use abomonation::Abomonation;
use abomonation_derive::Abomonation;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use caching_ext::*;
use itertools::Itertools;
use maplit::hashmap;
use maplit::hashset;
use memcache::KeyGen;
use retry::RetryLogic;
use retry::retry;
use sql_query_config::CachingConfig;

const RETRY_ATTEMPTS: usize = 2;

// This wraps around rust/shed/sql::queries, check that macro: https://fburl.com/code/semq9xm3
/// Define SQL queries that automatically retry on certain errors.
///
/// Caching can be enabled on a read query by:
/// - Adding "cacheable" keyword to your query.
/// - Make sure all parameters (input) to the query implement the Hash trait.
/// - Making sure the return values (output) implement Serialize, Deserialize, Abomonation,
///   and bincode::{Encode, Decode}.
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
        ) -> ($( $rtype:ty ),* $(,)*) { $q:expr_2021 }
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
        ) -> ($( $rtype:ty ),* $(,)*) { $q:expr_2021 }
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
        ) -> ($( $rtype:ty ),* $(,)*) { mysql($mysql_q:expr_2021) sqlite($sqlite_q:expr_2021) }
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

                // Not possible to retry query with transaction
                #[allow(unused_imports)]
                pub use [<$name Impl>]::query_with_transaction;
                #[allow(unused_imports)]
                use [<$name Impl>]::commented_query_with_transaction;

                #[allow(dead_code)]
                pub async fn traced_query_with_transaction(
                    transaction: Transaction,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    let cri = serde_json::to_string(cri)?;
                    commented_query_with_transaction(transaction, &cri, $( $pname, )* $( $lname, )*).await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query_with_transaction(
                    transaction: Transaction,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    match cri {
                        Some(cri) => traced_query_with_transaction(transaction, &cri, $( $pname, )* $( $lname, )*).await,
                        None => query_with_transaction(transaction, $( $pname, )* $( $lname, )*).await
                    }
                }

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    query_with_retry_no_cache(
                        || [<$name Impl>]::query(connection, $( $pname, )* $( $lname, )*),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn traced_query(
                    connection: &Connection,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    let cri = serde_json::to_string(cri)?;
                    query_with_retry_no_cache(
                        || [<$name Impl>]::commented_query(connection, &cri, $( $pname, )* $( $lname, )*),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query(
                    connection: &Connection,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    match cri {
                        Some(cri) => traced_query(connection, &cri, $( $pname, )* $( $lname, )*).await,
                        None => query(connection, $( $pname, )* $( $lname, )*).await
                    }
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
        ) -> ($( $rtype:ty ),* $(,)*) { mysql($mysql_q:expr_2021) sqlite($sqlite_q:expr_2021) }
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

                // Not possible to retry query with transaction
                #[allow(unused_imports)]
                pub use [<$name Impl>]::query_with_transaction;
                #[allow(unused_imports)]
                use [<$name Impl>]::commented_query_with_transaction;


                #[allow(dead_code)]
                pub async fn traced_query_with_transaction(
                    transaction: Transaction,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    let cri = serde_json::to_string(cri)?;
                    commented_query_with_transaction(transaction, &cri, $( $pname, )* $( $lname, )*).await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query_with_transaction(
                    transaction: Transaction,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, Vec<($( $rtype, )*)>)> {
                    match cri {
                        Some(cri) => traced_query_with_transaction(transaction, &cri, $( $pname, )* $( $lname, )*).await,
                        None => query_with_transaction(transaction, $( $pname, )* $( $lname, )*).await
                    }
                }

                #[allow(dead_code)]
                pub async fn query(
                    config: &SqlQueryConfig,
                    cache_ttl: Option<std::time::Duration>,
                    connection: &Connection,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
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


                    Ok(query_with_retry(
                        data,
                        || async move { Ok(CachedQueryResult([<$name Impl>]::query(connection, $( $pname, )* $( $lname, )*).await?)) },
                    ).await?.0)
                }

                #[allow(dead_code)]
                pub async fn traced_query(
                    config: &SqlQueryConfig,
                    cache_ttl: Option<std::time::Duration>,
                    connection: &Connection,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
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


                    let cri = serde_json::to_string(cri)?;
                    Ok(query_with_retry(
                        data,
                        || {
                        let cri = cri.clone();
                        async move {
                            Ok(CachedQueryResult([<$name Impl>]::commented_query(connection, &cri, $( $pname, )* $( $lname, )*).await?))
                        }
        },
                    ).await?.0)
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query(
                    config: &SqlQueryConfig,
                    cache_ttl: Option<std::time::Duration>,
                    connection: &Connection,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<Vec<($( $rtype, )*)>> {
                    match cri {
                        Some(cri) => traced_query(config, cache_ttl, connection, &cri, $( $pname, )* $( $lname, )*).await,
                        None => query(config, cache_ttl, connection, $( $pname, )* $( $lname, )*).await
                    }
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
        ) { $qtype:ident, $q:expr_2021 }
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
        ) { $qtype:ident, mysql($mysql_q:expr_2021) sqlite($sqlite_q:expr_2021) }
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

                // Not possible to retry query with transaction
                #[allow(unused_imports)]
                pub use [<$name Impl>]::query_with_transaction;
                #[allow(unused_imports)]
                use [<$name Impl>]::commented_query_with_transaction;


                #[allow(dead_code)]
                pub async fn traced_query_with_transaction(
                    transaction: Transaction,
                    cri: &ClientRequestInfo,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<(Transaction, WriteResult)> {
                    let cri = serde_json::to_string(cri)?;
                    commented_query_with_transaction(transaction, &cri, values $( , $pname )*)
                        .await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query_with_transaction(
                    transaction: Transaction,
                    cri: Option<&ClientRequestInfo>,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<(Transaction, WriteResult)> {
                    match cri {
                        Some(cri) => traced_query_with_transaction(transaction, &cri, values $( , $pname )*).await,
                        None => query_with_transaction(transaction, values $( , $pname )*).await
                    }
                }

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<WriteResult> {
                    query_with_retry_no_cache(
                        || [<$name Impl>]::query(connection, values $( , $pname )* ),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn traced_query(
                    connection: &Connection,
                    cri: &ClientRequestInfo,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<WriteResult> {
                    let cri = serde_json::to_string(cri)?;
                    query_with_retry_no_cache(
                        || [<$name Impl>]::commented_query(connection, &cri, values $( , $pname )* ),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query(
                    connection: &Connection,
                    cri: Option<&ClientRequestInfo>,
                    values: &[($( & $vtype, )*)],
                    $( $pname: & $ptype ),*
                ) -> Result<WriteResult> {
                    match cri {
                        Some(cri) => traced_query(connection, &cri, values $( , $pname )*).await,
                        None => query(connection, values $( , $pname )*).await
                    }
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
        ) { $qtype:ident, $q:expr_2021 }
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
        ) { $qtype:ident, mysql($mysql_q:expr_2021) sqlite($sqlite_q:expr_2021) }
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

                // Not possible to retry query with transaction
                #[allow(unused_imports)]
                pub use [<$name Impl>]::query_with_transaction;
                #[allow(unused_imports)]
                use [<$name Impl>]::commented_query_with_transaction;

                #[allow(dead_code)]
                pub async fn traced_query_with_transaction(
                    transaction: Transaction,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, WriteResult)> {
                    let cri = serde_json::to_string(cri)?;
                    commented_query_with_transaction(transaction, &cri $( , $pname )* $( , $lname )*)
                        .await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query_with_transaction(
                    transaction: Transaction,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<(Transaction, WriteResult)> {
                    match cri {
                        Some(cri) => traced_query_with_transaction(transaction, &cri $( , $pname )* $( , $lname )*).await,
                        None => query_with_transaction(transaction $( , $pname )* $( , $lname )*).await
                    }
                }

                #[allow(dead_code)]
                pub async fn query(
                    connection: &Connection,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<WriteResult> {
                    query_with_retry_no_cache(
                        || [<$name Impl>]::query(connection, $( $pname, )* $( $lname, )*),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn traced_query(
                    connection: &Connection,
                    cri: &ClientRequestInfo,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<WriteResult> {
                    let cri = serde_json::to_string(cri)?;
                    query_with_retry_no_cache(
                        || [<$name Impl>]::commented_query(connection, &cri, $( $pname, )* $( $lname, )*),
                    ).await
                }

                #[allow(dead_code)]
                pub async fn maybe_traced_query(
                    connection: &Connection,
                    cri: Option<&ClientRequestInfo>,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> Result<WriteResult> {
                    match cri {
                        Some(cri) => traced_query(connection, &cri, $( $pname, )* $( $lname, )*).await,
                        None => query(connection, $( $pname, )* $( $lname, )*).await
                    }
                }
            }

            $crate::mononoke_queries! { $( $rest )* }
        }
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

#[derive(Abomonation, Clone)]
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
) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>>,
{
    if let Ok(true) = justknobs::eval("scm/mononoke:sql_disable_auto_retries", None, None) {
        return do_query().await;
    }
    Ok(retry(
        None,
        |_| do_query(),
        should_retry_mysql_query,
        // See https://fburl.com/7dmedu1u for backoff reasoning
        RetryLogic::ExponentialWithJitter {
            base: Duration::from_secs(10),
            factor: 1.2,
            jitter: Duration::from_secs(5),
        },
        RETRY_ATTEMPTS,
    )
    .await?
    .0)
}

pub async fn query_with_retry<T, Fut>(
    cache_data: CacheData<'_>,
    do_query: impl Fn() -> Fut + Send + Sync,
) -> Result<CachedQueryResult<Vec<T>>>
where
    T: Send + Abomonation + bincode::Encode + bincode::Decode<()> + Clone + 'static,
    CachedQueryResult<Vec<T>>: MemcacheEntity,
    Fut: Future<Output = Result<CachedQueryResult<Vec<T>>>> + Send,
{
    if let Ok(true) = justknobs::eval("scm/mononoke:sql_disable_auto_cache", None, None) {
        return query_with_retry_no_cache(&do_query).await;
    }
    let fetch = || query_with_retry_no_cache(&do_query);
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

#[cfg(test)]
mod tests {
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
    async fn should_compile() -> anyhow::Result<()> {
        use clientinfo::ClientEntryPoint;
        use clientinfo::ClientRequestInfo;
        use sql_query_config::SqlQueryConfig;

        let config: &SqlQueryConfig = todo!();
        let connection: &sql::Connection = todo!();
        let cri = ClientRequestInfo::new(ClientEntryPoint::Sapling);
        TestQuery::query(connection, todo!(), todo!()).await?;
        TestQuery::query_with_transaction(todo!(), todo!(), todo!()).await?;
        TestQuery2::query(config, None, connection).await?;
        TestQuery2::query(config, Some(std::time::Duration::from_secs(60)), connection).await?;
        TestQuery2::query_with_transaction(todo!()).await?;
        TestQuery3::query(connection, &[(&12,)]).await?;
        TestQuery3::query_with_transaction(todo!(), &[(&12,)]).await?;
        TestQuery4::query(connection, &"hello").await?;
        TestQuery::traced_query(connection, &cri, todo!(), todo!()).await?;
        TestQuery2::traced_query(config, None, connection, &cri).await?;
        TestQuery3::traced_query(connection, &cri, &[(&12,)]).await?;
        TestQuery4::traced_query(connection, &cri, &"hello").await?;
        TestQuery::maybe_traced_query(connection, Some(&cri), todo!(), todo!()).await?;
        TestQuery2::maybe_traced_query(config, None, connection, Some(&cri)).await?;
        TestQuery3::maybe_traced_query(connection, Some(&cri), &[(&12,)]).await?;
        TestQuery4::maybe_traced_query(connection, Some(&cri), &"hello").await?;
        TestQuery::maybe_traced_query(connection, None, todo!(), todo!()).await?;
        TestQuery2::maybe_traced_query(config, None, connection, None).await?;
        TestQuery3::maybe_traced_query(connection, None, &[(&12,)]).await?;
        TestQuery4::maybe_traced_query(connection, None, &"hello").await?;
        Ok(())
    }
}

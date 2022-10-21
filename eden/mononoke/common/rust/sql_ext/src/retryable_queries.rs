/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use retry::retry;
use retry::RetryLogic;

const RETRY_ATTEMPTS: usize = 2;

#[macro_export]
macro_rules! queries_with_retry {
    () => {};

    (
        read $name:ident (
            $( $pname:ident: $ptype:ty ),* $(,)*
            $( >list $lname:ident: $ltype:ty )*
        ) -> ($( $rtype:ty ),* $(,)*) { $q:expr }
        $( $rest:tt )*
    ) => {

        $crate::_macro_internal::paste::item! {
            $crate::_macro_internal::queries! {
                read [<$name Impl>] (
                    $( $pname: $ptype ),*  ,
                    $( >list $lname: $ltype )*
                ) -> ($( $rtype ),*) { $q }
            }

            #[allow(non_snake_case)]
            mod $name {
                #[allow(unused_imports)]
                use super::*;

                #[allow(dead_code)]
                pub(super) async fn query(
                    connection: & $crate::_macro_internal::Connection,
                    $( $pname: & $ptype, )*
                    $( $lname: & [ $ltype ], )*
                ) -> $crate::_macro_internal::Result<Vec<($( $rtype, )*)>> {
                    $crate::_macro_internal::read_query_with_retry(
                        || [<$name Impl>]::query(connection, $( $pname, )* $( $lname, )*),
                    ).await
                }
            }

            $crate::queries_with_retry! { $( $rest )* }
        }
    };
}

#[cfg(fbcode_build)]
/// See https://fburl.com/sv/uk8w71td for error descriptions
fn retryable_mysql_errno(errno: u32) -> bool {
    match errno {
        // Admission control errors
        1914..=1916 => true,
        _ => false,
    }
}

#[cfg(fbcode_build)]
fn should_retry_mysql_query(err: &anyhow::Error) -> bool {
    use mysql_client::MysqlError;
    use MysqlError::*;
    match err.downcast_ref::<MysqlError>() {
        Some(ConnectionOperationError { mysql_errno, .. })
        | Some(QueryResultError { mysql_errno, .. }) => retryable_mysql_errno(*mysql_errno),
        _ => false,
    }
}

#[cfg(not(fbcode_build))]
fn should_retry_mysql_query(err: &anyhow::Error) -> bool {
    false
}

pub async fn read_query_with_retry<T, Fut>(mut do_query: impl FnMut() -> Fut + Send) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>>,
{
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

#[cfg(test)]
mod tests {

    queries_with_retry! {
        read TestQuery(param_str: String, param_uint: u64) -> (u64, Option<i32>, String, i64) {
            "SELECT 44, NULL, {param_str}, {param_uint}"
        }
        read TestQuery2() -> (u64, Option<String>) {
            "SELECT 44, NULL"
        }
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;

use anyhow::Result;

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

pub async fn read_query_with_retry<T, Fut>(mut do_query: impl FnMut() -> Fut + Send) -> Result<T>
where
    T: Send,
    Fut: Future<Output = Result<T>>,
{
    do_query().await
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

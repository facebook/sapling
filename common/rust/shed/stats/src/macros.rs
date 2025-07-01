/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

// This module defines only macros which don't show up on module level
// documentation anyway so hide it.
#![doc(hidden)]

#[doc(hidden)]
pub mod common_macro_prelude {
    pub use std::sync::Arc;
    pub use std::sync::LazyLock;
    pub use std::time::Duration;

    pub use perthread::PerThread;
    pub use perthread::ThreadMap;
    pub use stats_traits::dynamic_stat_types::DynamicStat;
    pub use stats_traits::dynamic_stat_types::DynamicStatSync;
    pub use stats_traits::field_stat_types::FieldStat;
    pub use stats_traits::field_stat_types::FieldStatThreadLocal;
    pub use stats_traits::stat_types::BoxCounter;
    pub use stats_traits::stat_types::BoxHistogram;
    pub use stats_traits::stat_types::BoxLocalCounter;
    pub use stats_traits::stat_types::BoxLocalHistogram;
    pub use stats_traits::stat_types::BoxLocalTimeseries;
    pub use stats_traits::stat_types::BoxSingletonCounter;
    pub use stats_traits::stat_types::BoxTimeseries;
    pub use stats_traits::stats_manager::AggregationType::*;
    pub use stats_traits::stats_manager::BoxStatsManager;
    pub use stats_traits::stats_manager::BucketConfig;
    pub use stats_traits::stats_manager::StatsManager;

    pub use crate::create_singleton_counter;
    pub use crate::create_stats_manager;
    pub use crate::thread_local_aggregator::create_map;
}

/// The macro to define STATS module that contains static variables, one per
/// counter you want to export. This is the main and recommended way to interact
/// with statistics provided by this crate. If non empty prefix is passed then
/// the exported counter name will be "{prefix}.{name}"
///
/// Examples:
/// ```standalone_crate
/// use stats::prelude::*;
/// use fbinit::FacebookInit;
///
/// define_stats! {
///     prefix = "my.test.counters";
///     manual_c: singleton_counter(),
///     test_c: counter(),
///     test_c2: counter("test_c.two"),
///     test_t: timeseries(Sum, Average),
///     test_t2: timeseries("test_t.two"; Sum, Average),
///     test_h: histogram(1, 0, 1000, Sum; P 99; P 50),
///     dtest_c: dynamic_counter("test_c.{}", (job: u64)),
///     dtest_t: dynamic_timeseries("test_t.{}", (region: &'static str); Rate, Sum),
///     dtest_t2: dynamic_timeseries("test_t.two.{}.{}", (job: u64, region: &'static str); Count),
///     dtest_h: dynamic_histogram("test_h.{}", (region: &'static str); 1, 0, 1000, Sum; P 99),
///     test_qs: quantile_stat("test_qs"; Count, Sum, Average; P 95, P 99; Duration::from_secs(60)),
///     test_qs_two: quantile_stat(Count, Sum, Average; P 95; Duration::from_secs(60)),
///     test_dynqs: dynamic_quantile_stat("test_dynqs_{}", (num: i32); Count, Sum, Average; P 95, P 99; Duration::from_secs(60)),
/// }
///
/// #[allow(non_snake_case)]
/// mod ALT_STATS {
///     use stats::define_stats;
///     define_stats! {
///         test_t: timeseries(Sum, Average),
///         test_t2: timeseries("test.two"; Sum, Average),
///     }
///     pub use self::STATS::*;
/// }
///
/// # #[allow(clippy::needless_doctest_main)]
/// #[fbinit::main]
/// fn main(fb: FacebookInit) {
///     STATS::manual_c.set_value(fb, 1);
///     STATS::test_c.increment_value(1);
///     STATS::test_c2.increment_value(100);
///     STATS::test_t.add_value(1);
///     STATS::test_t2.add_value_aggregated(79, 10);  // Add 79 and note it came from 10 samples
///     STATS::test_h.add_value(1);
///     STATS::test_h.add_repeated_value(1, 44);  // 44 times repeat adding 1
///     STATS::dtest_c.increment_value(7, (1000,));
///     STATS::dtest_t.add_value(77, ("lla",));
///     STATS::dtest_t2.add_value_aggregated(81, 12, (7, "lla"));
///     STATS::dtest_h.add_value(2, ("frc",));
///
///     ALT_STATS::test_t.add_value(1);
///     ALT_STATS::test_t2.add_value(1);
/// }
/// ```
///
/// # Reference
///
/// ## `singleton_counter`, `counter`
///
/// **DEPRECATED:** Use `timeseries` instead.
///
/// Raw counter types. These take an optional key parameter - if it is not
/// specified, then it's derived from the stat field name.
///
/// ## `timeseries`
/// The general syntax for `timeseries` is:
/// ```text
/// timeseries(<optional key>; <list of aggregations>; [optional intervals])
/// ```
///
/// If "optional key" is omitted then key is derived from the stat key name;
/// otherwise it's a string literal.
///
/// "List of aggregations" is a `,`-separated list of
/// [`AggregationType`](stats_traits::stats_manager::AggregationType) enum
/// values.
///
/// "Optional intervals" is a `,`-separated list of [`Duration`s](std::time::Duration). It
/// specifies over what time periods the aggregations aggregate. If not
/// specified, it typically defaults to 60 seconds.
///
/// This maps to a call to
/// [`StatsManager::create_timeseries`](stats_traits::stats_manager::StatsManager::create_histogram).
///
/// ## `histogram`
///
/// **DEPRECATED:** Use `quantile_stat` instead.
///
/// The general syntax for `histogram` is:
/// ```text
/// histogram(<optional key>; bucket-width, min, max, <list of aggregations>; <P XX percentiles>)
/// ```
/// If "optional key" is omitted then key is derived from the stat key name;
/// otherwise it's a string literal.
///
/// `bucket-width` specifies what range of values are accumulated into each
/// bucket; the larger it is the fewer buckets there are.
///
/// `min` and `max` specify the min and max of the expected range of samples.
/// Samples outside this range will be aggregated into out-of-range buckets,
/// which means they're not lost entirely but they lead to inaccurate stats.
///
/// Together the min, max and bucket width parameters determine how many buckets
/// are created.
///
/// "List of aggregations" is a `,`-separated list of
/// [`AggregationType`](stats_traits::stats_manager::AggregationType) enum
/// values.
///
/// Percentiles are specified as `P NN` in a `;`-separated list, such as `P 20;
/// P 50; P 90; P 99`...
///
/// This maps to a call to
/// [`StatsManager::create_histogram`](stats_traits::stats_manager::StatsManager::create_histogram).
///
/// ## `quantile_stat`
/// The general syntax for `quantile_stat` is:
/// ```text
/// quantile_stat(<optional key>; <list of aggregations>; <P XX percentiles>; <list of intervals>)
/// ```
///
/// "List of aggregations" is a `,`-separated list of
/// [`AggregationType`](stats_traits::stats_manager::AggregationType) enum
/// values.
///
/// Percentiles are specified as `P NN` or `P NN.N` in a `,`-separated list, such as `P 20,
/// P 50, P 90, P 99`...
/// You can also use floating point values as well such as `P 95.0, P 99.0, P 99.99`...
/// Please note that you provide "percentiles" instead of rates such as 0.95 like C++ API
///
/// "List of intervals" is a `,`-separated list of [`Duration`s](std::time::Duration). It
/// specifies over what time periods the aggregations aggregate.
///
/// `quantile_stat` measures the same statistics as `histogram`, but it
/// doesn't require buckets to be defined ahead of time. See [this workplace post](https://fb.workplace.com/notes/marc-celani/a-new-approach-to-quantile-estimation-in-c-services/212892662822481)
/// for more.
///
///  This maps to a call to
/// [`StatsManager::create_quantile_stat`](stats_traits::stats_manager::StatsManager::create_quantile_stat).
///
/// ## `dynamic_counter`, `dynamic_timeseries`, `dynamic_histogram`, `dynamic_quantile_stat`
///
/// These are equivalent to the corresponding `counter`/`timeseries`/`histogram`/`quantile_stat`
/// above, except that they allow the key to have a dynamic component. The key
/// is no longer optional, but is instead specified with `<format-string>,
/// (variable:type, ...)`. The format string is standard
/// [`format!`](std::format).
#[macro_export]
macro_rules! define_stats {
    // Fill the optional prefix with empty string, all matching is repeated here to avoid the
    // recursion limit reached error in case the macro is misused.
    ($( $name:ident: $stat_type:tt($( $params:tt )*), )*) =>
        (define_stats!(prefix = ""; $( $name: $stat_type($( $params )*), )*););

    (prefix = $prefix:expr_2021;
     $( $name:ident: $stat_type:tt($( $params:tt )*), )*) => (
        #[allow(non_snake_case, non_upper_case_globals, unused_imports, clippy::redundant_pub_crate)]
        pub(crate) mod STATS {
            use $crate::macros::common_macro_prelude::*;

            static STATS_MAP: LazyLock<Arc<ThreadMap<BoxStatsManager>>> = LazyLock::new(|| create_map());
            static STATS_MANAGER: LazyLock<BoxStatsManager> = LazyLock::new(|| create_stats_manager());

            thread_local! {
                static TL_STATS: PerThread<BoxStatsManager> =
                    STATS_MAP.register(create_stats_manager());
            }

            $( $crate::__define_stat!($prefix; $name: $stat_type($( $params )*)); )*
        }
    );
}

#[doc(hidden)]
#[macro_export]
macro_rules! __define_key_generator {
    ($name:ident($prefix:literal, $key:expr_2021; $( $placeholder:ident: $type:ty ),+)) => (
        fn $name(&($( ref $placeholder, )+): &($( $type, )+)) -> String {
            if $prefix.is_empty() {
                format!($key, $( $placeholder ),+)
            } else {
                format!(concat!($prefix, ".", $key), $( $placeholder ),+)
            }
        }
    );
    ($name:ident($prefix:expr_2021, $key:expr_2021)) => (
        fn $name() -> String {
            if $prefix.is_empty() {
                $key
            } else {
                format!("{0}.{1}", $prefix, $key)
            }
        }
    );
    ($name:ident($prefix:expr_2021, $key:expr_2021; $placeholder:ident: $type:ty )) => (
        fn $name(&( ref $placeholder, ): &( $type, )) -> String {
            if $prefix.is_empty() {
                format!($key, $placeholder)
            } else {
                format!(concat!("{1}.", $key), $placeholder, $prefix )
            }
        }
    );
    ($name:ident($prefix:expr_2021, $key:expr_2021; $placeholder1:ident: $type1:ty, $placeholder2:ident: $type2:ty )) => (
        fn $name(&( ref $placeholder1, ref $placeholder2, ): &( $type1, $type2, )) -> String {
            if $prefix.is_empty() {
                format!($key, $placeholder1, $placeholder2)
            } else {
                format!(concat!("{2}.", $key), $placeholder1, $placeholder2, $prefix )
            }
        }
    );
    ($name:ident($prefix:expr_2021, $key:expr_2021; $placeholder1:ident: $type1:ty, $placeholder2:ident: $type2:ty, $placeholder3:ident: $type3:ty )) => (
        fn $name(&( ref $placeholder1, ref $placeholder2, ref $placeholder3,): &( $type1, $type2, $type3,)) -> String {
            if $prefix.is_empty() {
                format!($key, $placeholder1, $placeholder2, $placeholder3)
            } else {
                format!(concat!("{3}.", $key), $placeholder1, $placeholder2, $placeholder3, $prefix )
            }
        }
    );
    ($name:ident($prefix:expr_2021, $key:expr_2021; $( $placeholder:ident: $type:ty ),+)) => (
        fn $name(&($( ref $placeholder, )+): &($( $type, )+)) -> String {
            let key = format!($key, $( $placeholder ),+);
            if $prefix.is_empty() {
                key
            } else {
                format!("{0}.{1}", $prefix, key)
            }
        }
    );

}

#[doc(hidden)]
#[macro_export]
macro_rules! __define_stat {
    ($prefix:expr_2021; $name:ident: singleton_counter()) => (
        $crate::__define_stat!($prefix; $name: singleton_counter(stringify!($name)));
    );

    ($prefix:expr_2021; $name:ident: singleton_counter($key:expr_2021)) => (
        pub static $name: LazyLock<BoxSingletonCounter> = LazyLock::new(|| create_singleton_counter($crate::__create_stat_key!($prefix, $key).to_string()));
    );

    ($prefix:expr_2021; $name:ident: counter()) => (
        $crate::__define_stat!($prefix; $name: counter(stringify!($name)));
    );

    ($prefix:expr_2021; $name:ident: counter($key:expr_2021)) => (
        thread_local! {
            pub static $name: BoxLocalCounter = TL_STATS.with(|stats| {
                stats.create_counter(&$crate::__create_stat_key!($prefix, $key))
            });
        }
    );

    // There are 4 inputs we use to produce a timeseries: the the prefix, the name (used in
    // STATS::name), the key (used in ODS or to query the key), the export types (SUM, RATE, etc.),
    // and the intervals (e.g. 60, 600). The key defaults to the name, and the intervals default to
    // whatever default Folly uses (which happens to be 60, 600, 3600);
    ($prefix:expr_2021; $name:ident: timeseries($( $aggregation_type:expr_2021 ),*)) => (
        $crate::__define_stat!($prefix; $name: timeseries(stringify!($name); $( $aggregation_type ),*));
    );
    ($prefix:expr_2021; $name:ident: timeseries($key:expr_2021; $( $aggregation_type:expr_2021 ),*)) => (
        $crate::__define_stat!($prefix; $name: timeseries($key; $( $aggregation_type ),* ; ));
    );
    ($prefix:expr_2021; $name:ident: timeseries($key:expr_2021; $( $aggregation_type:expr_2021 ),* ; $( $interval: expr_2021 ),*)) => (
        thread_local! {
            pub static $name: BoxLocalTimeseries = TL_STATS.with(|stats| {
                stats.create_timeseries(
                    &$crate::__create_stat_key!($prefix, $key),
                    &[$( $aggregation_type ),*],
                    &[$( $interval ),*]
                )
            });
        }
    );

    ($prefix:expr_2021;
     $name:ident: histogram($bucket_width:expr_2021,
                            $min:expr_2021,
                            $max:expr_2021
                            $(, $aggregation_type:expr_2021 )*
                            $(; P $percentile:expr_2021 )*)) => (
        $crate::__define_stat!($prefix;
                      $name: histogram(stringify!($name);
                                       $bucket_width,
                                       $min,
                                       $max
                                       $(, $aggregation_type )*
                                       $(; P $percentile )*));
    );

    ($prefix:expr_2021;
     $name:ident: histogram($key:expr_2021;
                            $bucket_width:expr_2021,
                            $min:expr_2021,
                            $max:expr_2021
                            $(, $aggregation_type:expr_2021 )*
                            $(; P $percentile:expr_2021 )*)) => (
        thread_local! {
            pub static $name: BoxLocalHistogram = TL_STATS.with(|stats| {
                stats.create_histogram(
                    &$crate::__create_stat_key!($prefix, $key),
                    &[$( $aggregation_type ),*],
                    BucketConfig {
                        width: $bucket_width,
                        min: $min,
                        max: $max,
                    },
                    &[$( $percentile ),*])
            });
        }
    );

    ($prefix:expr_2021;
        $name:ident: quantile_stat(
            $( $aggregation_type:expr_2021 ),*
            ; $( P $percentile:expr_2021 ),*
            ; $( $interval:expr_2021 ),*
        )) => (
            $crate::__define_stat!($prefix;
                 $name: quantile_stat(stringify!($name)
                                     ; $( $aggregation_type ),*
                                     ; $( P $percentile ),*
                                     ; $( $interval ),*));
       );

    ($prefix:expr_2021;
        $name:ident: quantile_stat($key:expr_2021
            ; $( $aggregation_type:expr_2021 ),*
            ; $( P $percentile:expr_2021 ),*
            ; $( $interval:expr_2021 ),*
        )) => (
                pub static $name: LazyLock<BoxHistogram> = LazyLock::new(|| {
                    STATS_MANAGER.create_quantile_stat(
                        &$crate::__create_stat_key!($prefix, $key),
                        &[$( $aggregation_type ),*],
                        &[$( $percentile as f32 ),*],
                        &[$( $interval ),*],
                    )
                });

       );

    ($prefix:expr_2021;
     $name:ident: dynamic_singleton_counter($key:expr_2021, ($( $placeholder:ident: $type:ty ),+))) => (
        thread_local! {
            pub static $name: DynamicStat<($( $type, )+), BoxSingletonCounter> = {
                $crate::__define_key_generator!(
                    __key_generator($prefix, $key; $( $placeholder: $type ),+)
                );

                fn __stat_generator(key: &str) -> BoxSingletonCounter {
                    create_singleton_counter(key.to_string())
                }

                DynamicStat::new(__key_generator, __stat_generator)
            }
        }
    );

    ($prefix:expr_2021;
     $name:ident: dynamic_counter($key:expr_2021, ($( $placeholder:ident: $type:ty ),+))) => (
        thread_local! {
            pub static $name: DynamicStat<($( $type, )+), BoxLocalCounter> = {
                $crate::__define_key_generator!(
                    __key_generator($prefix, $key; $( $placeholder: $type ),+)
                );

                fn __stat_generator(key: &str) -> BoxLocalCounter {
                    TL_STATS.with(|stats| {
                        stats.create_counter(key)
                    })
                }

                DynamicStat::new(__key_generator, __stat_generator)
            }
        }
    );

    ($prefix:expr_2021;
     $name:ident: dynamic_timeseries($key:expr_2021, ($( $placeholder:ident: $type:ty ),+);
                                     $( $aggregation_type:expr_2021 ),*)) => (
        $crate::__define_stat!(
            $prefix;
            $name: dynamic_timeseries(
                $key,
                ($( $placeholder: $type ),+);
                $( $aggregation_type ),* ;
            )
        );
    );

    ($prefix:expr_2021;
     $name:ident: dynamic_timeseries($key:expr_2021, ($( $placeholder:ident: $type:ty ),+);
                                     $( $aggregation_type:expr_2021 ),* ; $( $interval:expr_2021 ),*)) => (
        thread_local! {
            pub static $name: DynamicStat<($( $type, )+), BoxLocalTimeseries> = {
                $crate::__define_key_generator!(
                    __key_generator($prefix, $key; $( $placeholder: $type ),+)
                );

                fn __stat_generator(key: &str) -> BoxLocalTimeseries {
                    TL_STATS.with(|stats| {
                        stats.create_timeseries(key, &[$( $aggregation_type ),*], &[$( $interval ),*])
                    })
                }

                DynamicStat::new(__key_generator, __stat_generator)
            };
        }
    );

    ($prefix:expr_2021;
     $name:ident: dynamic_histogram($key:expr_2021, ($( $placeholder:ident: $type:ty ),+);
                                    $bucket_width:expr_2021,
                                    $min:expr_2021,
                                    $max:expr_2021
                                    $(, $aggregation_type:expr_2021 )*
                                    $(; P $percentile:expr_2021 )*)) => (
        thread_local! {
            pub static $name: DynamicStat<($( $type, )+), BoxLocalHistogram> = {
                $crate::__define_key_generator!(
                    __key_generator($prefix, $key; $( $placeholder: $type ),+)
                );

                fn __stat_generator(key: &str) -> BoxLocalHistogram {
                    TL_STATS.with(|stats| {
                        stats.create_histogram(key,
                                               &[$( $aggregation_type ),*],
                                               BucketConfig {
                                                   width: $bucket_width,
                                                   min: $min,
                                                   max: $max,
                                               },
                                               &[$( $percentile ),*])
                    })
                }

                DynamicStat::new(__key_generator, __stat_generator)
            };
        }
    );

    ($prefix:expr_2021;
     $name:ident: dynamic_quantile_stat($key:expr_2021, ($( $placeholder:ident: $type:ty ),+) ;
                                        $( $aggregation_type:expr_2021 ),* ;
                                        $( P $percentile:expr_2021 ),* ;
                                        $( $interval:expr_2021 ),*)) => (
                pub static $name: LazyLock<DynamicStatSync<($( $type, )+), BoxHistogram>> = LazyLock::new(|| {
                    $crate::__define_key_generator!(
                        __key_generator($prefix, $key; $( $placeholder: $type ),+)
                    );

                    fn __stat_generator(key: &str) -> BoxHistogram {
                        STATS_MANAGER.create_quantile_stat(
                            key,
                            &[$( $aggregation_type ),*],
                            &[$( $percentile as f32 ),*],
                            &[$( $interval ),*],
                        )
                    }
                    DynamicStatSync::new(__key_generator, __stat_generator)
                });
       );
}

#[doc(hidden)]
#[macro_export]
macro_rules! __create_stat_key {
    ($prefix:expr_2021, $key:expr_2021) => {{
        use std::borrow::Cow;
        if $prefix.is_empty() {
            Cow::Borrowed($key)
        } else {
            Cow::Owned(format!("{}.{}", $prefix, $key))
        }
    }};
}

/// Define a group of stats with dynamic names all parameterized by the same set of parameters.
/// The intention is that when setting up a structure for some entity with associated stats, then
/// the type produced by this macro can be included in that structure, and initialized with the
/// appropriate name(s). This is more efficient than using single static "dynamic_" versions of
/// the counters.
///
/// ```
/// use stats::prelude::*;
///
/// define_stats_struct! {
///    // struct name, key prefix template, key template params
///    MyThingStat("things.{}.{}", mything_name: String, mything_idx: usize),
///    cache_miss: counter() // default name from the field
/// }
///
/// struct MyThing {
///     stats: MyThingStat,
/// }
///
/// impl MyThing {
///     fn new(somename: String, someidx: usize) -> Self {
///         MyThing {
///             stats: MyThingStat::new(somename, someidx),
///             //...
///         }
///     }
/// }
/// #
/// # fn main() {}
/// ```
#[macro_export]
macro_rules! define_stats_struct {
    // Handle trailing comma
    ($name:ident ($key:expr_2021, $($pr_name:ident: $pr_type:ty),*) ,
        $( $stat_name:ident: $stat_type:tt($( $params:tt )*) , )+) => {
        define_stats_struct!($name ( $key, $($pr_name: $pr_type),*),
            $($stat_name: $stat_type($($params)*)),* );
    };

    // Handle no params
    ($name:ident ($key:expr_2021) ,
        $( $stat_name:ident: $stat_type:tt($( $params:tt )*) ),*) => {
        define_stats_struct!($name ( $key, ),
            $($stat_name: $stat_type($($params)*)),* );
    };
    ($name:ident ($key:expr_2021) ,
        $( $stat_name:ident: $stat_type:tt($( $params:tt )*) , )+) => {
        define_stats_struct!($name ( $key, ),
            $($stat_name: $stat_type($($params)*)),* );
    };

    // Define struct and its methods.
    ($name:ident ($key:expr_2021, $($pr_name:ident: $pr_type:ty),*) ,
        $( $stat_name:ident: $stat_type:tt($( $params:tt )*) ),*) => {
        #[allow(missing_docs)]
        pub struct $name {
            $(pub $stat_name: $crate::__struct_field_type!($stat_type), )*
        }
        impl $name {
            #[allow(unused_imports, missing_docs, non_upper_case_globals)]
            pub fn new($($pr_name: $pr_type),*) -> $name {
                use $crate::macros::common_macro_prelude::*;


                static STATS_MAP: LazyLock<Arc<ThreadMap<BoxStatsManager>>> = LazyLock::new(|| create_map());
                static STATS_MANAGER: LazyLock<BoxStatsManager> = LazyLock::new(|| create_stats_manager());

                thread_local! {
                    static TL_STATS: PerThread<BoxStatsManager> =
                        STATS_MAP.register(create_stats_manager());
                }

                $(
                    $crate::__struct_thread_local_init! { $stat_name, $stat_type, $($params)* }
                )*

                let __prefix = format!($key, $($pr_name),*);

                $name {
                    $($stat_name: $crate::__struct_field_init!(__prefix, $stat_name, $stat_type, $($params)*)),*
                }
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(fmt, "<{}>", stringify!($name))
            }
        }
    }
}

#[macro_export]
#[doc(hidden)]
macro_rules! __struct_field_type {
    (singleton_counter) => {
        $crate::macros::common_macro_prelude::BoxSingletonCounter
    };
    (counter) => {
        $crate::macros::common_macro_prelude::BoxCounter
    };
    (timeseries) => {
        $crate::macros::common_macro_prelude::BoxTimeseries
    };
    (histogram) => {
        $crate::macros::common_macro_prelude::BoxHistogram
    };
    (quantile_stat) => {
        $crate::macros::common_macro_prelude::BoxHistogram
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __struct_thread_local_init {
    ($name:ident, singleton_counter, ) => {
        $crate::__struct_thread_local_init!($name, singleton_counter, stringify!($name))
    };
    ($name:ident, singleton_counter, $key:expr_2021) => {};

    ($name:ident, counter, ) => {
        $crate::__struct_thread_local_init! { $name, counter, stringify!($name)}
    };
    ($name:ident, counter, $key:expr_2021) => {
        $crate::__struct_thread_local_init! { $name, counter, $key ; }
    };
    ($name:ident, counter, $key:expr_2021 ; ) => {
        thread_local! {
            static $name: FieldStatThreadLocal<BoxLocalCounter> = {

                fn __stat_generator(key: &str) -> BoxLocalCounter {
                    TL_STATS.with(|stats| {
                        stats.create_counter(key)
                    })
                }

                FieldStatThreadLocal::new(__stat_generator)
            };
        }
    };

    ($name:ident, timeseries, $( $aggregation_type:expr_2021 ),+) => {
        $crate::__struct_thread_local_init! { $name, timeseries, stringify!($name) ; $($aggregation_type),*}
    };
    ($name:ident, timeseries, $key:expr_2021 ; $( $aggregation_type:expr_2021 ),* ) => {
        $crate::__struct_thread_local_init! { $name, timeseries, $key ; $($aggregation_type),* ;}
    };
    ($name:ident, timeseries, $key:expr_2021 ; $( $aggregation_type:expr_2021 ),* ; $( $interval:expr_2021 ),* ) => {
        thread_local! {

            static $name: FieldStatThreadLocal<BoxLocalTimeseries> = {

                fn __stat_generator(key: &str) -> BoxLocalTimeseries {
                    TL_STATS.with(|stats| {
                        stats.create_timeseries(key, &[$( $aggregation_type ),*], &[$( $interval ),*])
                    })
                }

                FieldStatThreadLocal::new(__stat_generator)
            };
        }
    };

    ($name:ident, histogram,
        $bucket_width:expr_2021, $min:expr_2021, $max:expr_2021 $(, $aggregation_type:expr_2021)*
        $(; P $percentile:expr_2021 )*) => {
        $crate::__struct_thread_local_init! { $name, histogram,
            stringify!($name) ; $bucket_width, $min, $max $(, $aggregation_type)*
            $(; P $percentile)* }
    };
    ($name:ident, histogram, $key:expr_2021 ;
        $bucket_width:expr_2021, $min:expr_2021, $max:expr_2021 $(, $aggregation_type:expr_2021)*
        $(; P $percentile:expr_2021 )*) => {

        thread_local! {
            static $name: FieldStatThreadLocal<BoxLocalHistogram> = {

                fn __stat_generator(key: &str) -> BoxLocalHistogram {
                    TL_STATS.with(|stats| {
                        stats.create_histogram(key,
                                               &[$( $aggregation_type ),*],
                                               BucketConfig {
                                                   width: $bucket_width,
                                                   min: $min,
                                                   max: $max,
                                               },
                                               &[$( $percentile ),*])
                    })
                }

                FieldStatThreadLocal::new(__stat_generator)
            };
        }
    };
    ($name:ident, quantile_stat,
        $( $aggregation_type:expr_2021 ),*
        ; $( P $percentile:expr_2021 ),*
        ; $( $interval:expr_2021 ),*) => ();
    ($name:ident, quantile_stat, $key:expr_2021
        ; $( $aggregation_type:expr_2021 ),*
        ; $( P $percentile:expr_2021 ),*
        ; $( $interval:expr_2021 ),*) => ();
}

#[macro_export]
#[doc(hidden)]
macro_rules! __struct_field_init {
    ($prefix:expr_2021, $name:ident, singleton_counter, ) => {
        $crate::__struct_field_init!($prefix, $name, singleton_counter, stringify!($name))
    };
    ($prefix:expr_2021, $name:ident, singleton_counter, $key:expr_2021) => {{ create_singleton_counter(format!("{}.{}", $prefix, $key)) }};

    ($prefix:expr_2021, $name:ident, counter, ) => {
        $crate::__struct_field_init!($prefix, $name, counter, stringify!($name) ;)
    };
    ($prefix:expr_2021, $name:ident, counter, $key:expr_2021) => {
        $crate::__struct_field_init!($prefix, $name, counter, $key ;)
    };
    ($prefix:expr_2021, $name:ident, counter, $key:expr_2021 ; $(params:tt)*) => {{ Box::new(FieldStat::new(&$name, format!("{}.{}", $prefix, $key))) }};


    ($prefix:expr_2021, $name:ident, timeseries, $( $aggregation_type:expr_2021 ),+) => {
        $crate::__struct_field_init!($prefix, $name, timeseries, stringify!($name) ; $($aggregation_type),*)
    };
    ($prefix:expr_2021, $name:ident, timeseries, $key:expr_2021 ; $( $aggregation_type:expr_2021 ),* ) => {
        $crate::__struct_field_init!($prefix, $name, timeseries, $key ; $($aggregation_type),* ;)
    };
    ($prefix:expr_2021, $name:ident, timeseries, $key:expr_2021 ; $( $aggregation_type:expr_2021 ),* ; $( $interval:expr_2021 ),* ) => {{
        Box::new(FieldStat::new(&$name, format!("{}.{}", $prefix, $key)))
    }};

    ($prefix:expr_2021, $name:ident, histogram,
        $bucket_width:expr_2021, $min:expr_2021, $max:expr_2021 $(, $aggregation_type:expr_2021)*
        $(; P $percentile:expr_2021 )*) => {
        $crate::__struct_field_init!($prefix, $name, histogram,
            stringify!($name) ; $bucket_width, $min, $max $(, $aggregation_type)*
            $(; P $percentile)*)
    };
    ($prefix:expr_2021, $name:ident, histogram, $key:expr_2021 ;
        $bucket_width:expr_2021, $min:expr_2021, $max:expr_2021 $(, $aggregation_type:expr_2021)*
        $(; P $percentile:expr_2021 )*) => {{ Box::new(FieldStat::new(&$name, format!("{}.{}", $prefix, $key))) }};
    ($prefix:expr_2021, $name:ident, quantile_stat,
        $( $aggregation_type:expr_2021 ),*
        ; $( P $percentile:expr_2021 ),*
        ; $( $interval:expr_2021 ),*) => {
            $crate::__struct_field_init!($prefix, $name, quantile_stat,
                stringify!($name)
                ; $( $aggregation_type ),*
                ; $( P $percentile ),*
                ; $( $interval ),*)
    };
    ($prefix:expr_2021, $name:ident, quantile_stat,
        $key:expr_2021
        ; $( $aggregation_type:expr_2021 ),*
        ; $( P $percentile:expr_2021 ),*
        ; $( $interval:expr_2021 ),*) => {{
            STATS_MANAGER.create_quantile_stat(
                &$crate::__create_stat_key!($prefix, $key),
                &[$( $aggregation_type ),*],
                &[$( $percentile as f32 ),*],
                &[$( $interval ),*],
            )

    }};
}

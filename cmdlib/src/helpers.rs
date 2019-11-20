/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::{cmp::min, fs, path::Path, str::FromStr, time::Duration};

use cloned::cloned;
use failure_ext::{bail_msg, err_msg, format_err, Error, Result, ResultExt};
use fbinit::FacebookInit;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use slog::{debug, info};
use upload_trace::{manifold_thrift::thrift::RequestContext, UploadTrace};

use blobrepo::BlobRepo;
use blobrepo_factory::ReadOnlyStorage;
use bookmarks::BookmarkName;
use changesets::SqlConstructors;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use metaconfig_types::MetadataDBConfig;
use mononoke_types::ChangesetId;

pub fn upload_and_show_trace(ctx: CoreContext) -> impl Future<Item = (), Error = !> {
    if !ctx.trace().is_enabled() {
        debug!(ctx.logger(), "Trace is disabled");
        return Ok(()).into_future().left_future();
    }

    let rc = RequestContext {
        bucketName: "mononoke_prod".into(),
        apiKey: "".into(),
        ..Default::default()
    };

    ctx.trace()
        .upload_to_manifold(rc)
        .then(move |upload_res| {
            match upload_res {
                Err(err) => debug!(ctx.logger(), "Failed to upload trace: {:#?}", err),
                Ok(()) => debug!(
                    ctx.logger(),
                    "Trace taken: https://our.intern.facebook.com/intern/mononoke/trace/{}",
                    ctx.trace().id()
                ),
            }
            Ok(())
        })
        .right_future()
}

pub fn setup_repo_dir<P: AsRef<Path>>(data_dir: P, create: bool) -> Result<()> {
    let data_dir = data_dir.as_ref();

    if !data_dir.is_dir() {
        bail_msg!("{:?} does not exist or is not a directory", data_dir);
    }

    for subdir in &["blobs"] {
        let subdir = data_dir.join(subdir);

        if subdir.exists() && !subdir.is_dir() {
            bail_msg!("{:?} already exists and is not a directory", subdir);
        }

        if create {
            if subdir.exists() {
                let content: Vec<_> = subdir.read_dir()?.collect();
                if !content.is_empty() {
                    bail_msg!(
                        "{:?} already exists and is not empty: {:?}",
                        subdir,
                        content
                    );
                }
            } else {
                fs::create_dir(&subdir)
                    .with_context(|_| format!("failed to create subdirectory {:?}", subdir))?;
            }
        }
    }
    Ok(())
}

pub struct CachelibSettings {
    pub cache_size: usize,
    pub max_process_size_gib: Option<u32>,
    pub min_process_size_gib: Option<u32>,
    pub use_tupperware_shrinker: bool,
    pub presence_cache_size: Option<usize>,
    pub changesets_cache_size: Option<usize>,
    pub filenodes_cache_size: Option<usize>,
    pub idmapping_cache_size: Option<usize>,
    pub with_content_sha1_cache: bool,
    pub content_sha1_cache_size: Option<usize>,
    pub blob_cache_size: Option<usize>,
}

impl Default for CachelibSettings {
    fn default() -> Self {
        Self {
            cache_size: 20 * 1024 * 1024 * 1024,
            max_process_size_gib: None,
            min_process_size_gib: None,
            use_tupperware_shrinker: false,
            presence_cache_size: None,
            changesets_cache_size: None,
            filenodes_cache_size: None,
            idmapping_cache_size: None,
            with_content_sha1_cache: false,
            content_sha1_cache_size: None,
            blob_cache_size: None,
        }
    }
}

pub fn init_cachelib_from_settings(fb: FacebookInit, settings: CachelibSettings) -> Result<()> {
    // Millions of lookups per second
    let lock_power = 10;
    // Assume 200 bytes average cache item size and compute bucketsPower
    let expected_item_size_bytes = 200;
    let cache_size_bytes = settings.cache_size;
    let item_count = cache_size_bytes / expected_item_size_bytes;

    // Because `bucket_count` is a power of 2, bucket_count.trailing_zeros() is log2(bucket_count)
    let bucket_count = item_count
        .checked_next_power_of_two()
        .ok_or_else(|| err_msg("Cache has too many objects to fit a `usize`?!?"))?;
    let buckets_power = min(bucket_count.trailing_zeros() + 1 as u32, 32);

    let mut cache_config = cachelib::LruCacheConfig::new(cache_size_bytes)
        .set_pool_rebalance(cachelib::PoolRebalanceConfig {
            interval: Duration::new(300, 0),
            strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                // A small increase in hit ratio is desired
                diff_ratio: 0.05,
                min_retained_slabs: 1,
                // Objects newer than 30 seconds old might be about to become interesting
                min_tail_age: Duration::new(30, 0),
                ignore_untouched_slabs: false,
            },
        })
        .set_access_config(buckets_power, lock_power);

    if settings.use_tupperware_shrinker {
        if settings.max_process_size_gib.is_some() || settings.min_process_size_gib.is_some() {
            return Err(err_msg(
                "Can't use both Tupperware shrinker and manually configured shrinker",
            ));
        }
        cache_config = cache_config.set_tupperware_shrinker();
    } else {
        match (settings.max_process_size_gib, settings.min_process_size_gib) {
            (None, None) => (),
            (Some(_), None) | (None, Some(_)) => {
                return Err(err_msg(
                    "If setting process size limits, must set both max and min",
                ));
            }
            (Some(max), Some(min)) => {
                cache_config = cache_config.set_shrinker(cachelib::ShrinkMonitor {
                    shrinker_type: cachelib::ShrinkMonitorType::ResidentSize {
                        max_process_size_gib: max,
                        min_process_size_gib: min,
                    },
                    interval: Duration::new(10, 0),
                    max_resize_per_iteration_percent: 25,
                    max_removed_percent: 50,
                    strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                        // A small increase in hit ratio is desired
                        diff_ratio: 0.05,
                        min_retained_slabs: 1,
                        // Objects newer than 30 seconds old might be about to become interesting
                        min_tail_age: Duration::new(30, 0),
                        ignore_untouched_slabs: false,
                    },
                });
            }
        };
    }

    cachelib::init_cache_once(fb, cache_config)?;
    cachelib::init_cacheadmin("mononoke")?;

    // Give each cache 5% of the available space, bar the blob cache which gets everything left
    // over. We can adjust this with data.
    let available_space = cachelib::get_available_space()?;
    cachelib::get_or_create_volatile_pool(
        "blobstore-presence",
        settings.presence_cache_size.unwrap_or(available_space / 20),
    )?;

    cachelib::get_or_create_volatile_pool(
        "changesets",
        settings
            .changesets_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "filenodes",
        settings
            .filenodes_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "bonsai_hg_mapping",
        settings
            .idmapping_cache_size
            .unwrap_or(available_space / 20),
    )?;

    if settings.with_content_sha1_cache {
        cachelib::get_or_create_volatile_pool(
            "content-sha1",
            settings
                .content_sha1_cache_size
                .unwrap_or(available_space / 20),
        )?;
    }

    cachelib::get_or_create_volatile_pool(
        "blobstore-blobs",
        settings
            .blob_cache_size
            .unwrap_or(cachelib::get_available_space()?),
    )?;

    Ok(())
}

/// Resovle changeset id by either bookmark name, hg hash, or changset id hash
pub fn csid_resolve(
    ctx: CoreContext,
    repo: BlobRepo,
    hash_or_bookmark: impl ToString,
) -> impl Future<Item = ChangesetId, Error = Error> {
    let hash_or_bookmark = hash_or_bookmark.to_string();
    BookmarkName::new(hash_or_bookmark.clone())
        .into_future()
        .and_then({
            cloned!(repo, ctx);
            move |name| repo.get_bonsai_bookmark(ctx, &name)
        })
        .and_then(|csid| csid.ok_or(err_msg("invalid bookmark")))
        .or_else({
            cloned!(ctx, repo, hash_or_bookmark);
            move |_| {
                HgChangesetId::from_str(&hash_or_bookmark)
                    .into_future()
                    .and_then(move |hg_csid| repo.get_bonsai_from_hg(ctx, hg_csid))
                    .and_then(|csid| csid.ok_or(err_msg("invalid hg changeset")))
            }
        })
        .or_else({
            cloned!(hash_or_bookmark);
            move |_| ChangesetId::from_str(&hash_or_bookmark)
        })
        .inspect(move |csid| {
            info!(ctx.logger(), "changeset resolved as: {:?}", csid);
        })
        .map_err(move |_| {
            format_err!(
                "invalid (hash|bookmark) or does not exist in this repository: {}",
                hash_or_bookmark
            )
        })
}

pub fn open_sql_with_config_and_myrouter_port<T>(
    dbconfig: MetadataDBConfig,
    maybe_myrouter_port: Option<u16>,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<T, Error>
where
    T: SqlConstructors,
{
    let name = T::LABEL;
    match dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            T::with_sqlite_path(path.join("sqlite_dbs"), readonly_storage.0)
                .into_future()
                .boxify()
        }
        MetadataDBConfig::Mysql { db_address, .. } if name != "filenodes" => {
            T::with_xdb(db_address, maybe_myrouter_port, readonly_storage.0)
        }
        MetadataDBConfig::Mysql { .. } => Err(err_msg(
            "Use SqlFilenodes::with_sharded_myrouter for filenodes",
        ))
        .into_future()
        .boxify(),
    }
}

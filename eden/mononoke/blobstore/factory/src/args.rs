/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU32;
use std::num::NonZeroUsize;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use arg_extensions::ArgDefaults;
use clap::Args;
use metaconfig_types::PackFormat;
use rand_distr::Normal;

use crate::PutBehaviour;

/// Options for controlling the blobstore
#[derive(Args, Debug)]
pub struct BlobstoreArgs {
    /// Read QPS limit
    #[clap(long)]
    pub blobstore_read_qps: Option<NonZeroU32>,

    /// Write QPS limit
    #[clap(long)]
    pub blobstore_write_qps: Option<NonZeroU32>,

    /// Read bytes/s limit
    #[clap(long)]
    pub blobstore_read_bytes_s: Option<NonZeroUsize>,

    /// Write bytes/s limit
    #[clap(long)]
    pub blobstore_write_bytes_s: Option<NonZeroUsize>,

    /// Read burst bytes/s limit
    #[clap(long)]
    pub blobstore_read_burst_bytes_s: Option<NonZeroUsize>,

    /// Write burst bytes/s limit
    #[clap(long)]
    pub blobstore_write_burst_bytes_s: Option<NonZeroUsize>,

    /// Minimum number of bytes ThrottledBlob can count
    #[clap(long)]
    pub blobstore_bytes_min_throttle: Option<NonZeroUsize>,

    /// Rate of errors on reads.  For value N, it will error randomly
    /// 1/N times.  For multiplexed stores, this will only apply to the
    /// first store in the multiplex.
    #[clap(long)]
    pub blobstore_read_chaos_rate: Option<NonZeroU32>,

    /// Rate of errors on writes.  For value N, it will error randomly
    /// 1/N times.  For multiplexed stores, this will only apply to the
    /// first store in the multiplex.
    #[clap(long)]
    pub blobstore_write_chaos_rate: Option<NonZeroU32>,

    /// Mean value of additional delay for blobstore put calls.
    #[clap(long, requires = "blobstore-put-stddev-delay-secs")]
    pub blobstore_put_mean_delay_secs: Option<f64>,

    /// Standard devation of additional delay for blobstore put calls.
    #[clap(long, requires = "blobstore-put-mean-delay-secs")]
    pub blobstore_put_stddev_delay_secs: Option<f64>,

    /// Mean value of additional delay for blobstore get calls.
    #[clap(long, requires = "blobstore-get-stddev-delay-secs")]
    pub blobstore_get_mean_delay_secs: Option<f64>,

    /// Standard deviation of additional delay for blobstore get calls.
    #[clap(long, requires = "blobstore-get-mean-delay-secs")]
    pub blobstore_get_stddev_delay_secs: Option<f64>,

    /// Override config to enable or disable zstd compression on write
    /// via PackBlob
    // This is Option<bool> as we distinguish between option being
    // not present vs being set to false.
    #[clap(long, value_name = "BOOL")]
    pub blobstore_write_zstd: Option<bool>,

    /// Override config to set the zstd compression level for writes
    /// via PackBlob.
    #[clap(long)]
    pub blobstore_write_zstd_level: Option<i32>,

    /// Whether to attempt zstd compression when the blobstore is putting
    /// things into cachelib over the threshold size.
    // For compatibility with existing usage, this arg takes value,
    // for example `--blobstore-cachelib-attempt-zstd=true`.
    #[clap(
        long,
        parse(try_from_str),
        default_value_t = false,
        value_name = "BOOL"
    )]
    pub blobstore_cachelib_attempt_zstd: bool,

    /// Desired blobstore behaviour when a put is made to an existing key.
    #[clap(long)]
    pub blobstore_put_behaviour: Option<PutBehaviour>,
}

impl BlobstoreArgs {
    pub fn put_format_override(&self) -> Result<Option<PackFormat>> {
        match (self.blobstore_write_zstd, self.blobstore_write_zstd_level) {
            (None, None) => Ok(None),
            (Some(false), None) => Ok(Some(PackFormat::Raw)),
            (Some(true), Some(level)) => Ok(Some(PackFormat::ZstdIndividual(level))),
            (Some(true), None) => Err(anyhow!(
                "Invalid arguments: --blobstore-write-zstd=true requires --blobstore-write-zstd-level"
            )),
            (_, Some(level)) => Err(anyhow!(
                "Invalid arguments: --blobstore-write-zstd-level={} requires --blobstore-write-zstd=true",
                level
            )),
        }
    }

    pub fn get_delay_distribution(&self) -> Result<Option<Normal<f64>>> {
        delay_distribution(
            self.blobstore_get_mean_delay_secs,
            self.blobstore_get_stddev_delay_secs,
        )
        .context("Failed to create blobstore get delay distribution")
    }

    pub fn put_delay_distribution(&self) -> Result<Option<Normal<f64>>> {
        delay_distribution(
            self.blobstore_put_mean_delay_secs,
            self.blobstore_put_stddev_delay_secs,
        )
        .context("Failed to create blobstore put delay distribution")
    }
}

#[derive(Default, Debug)]
pub struct BlobstoreArgDefaults {
    pub read_qps: Option<NonZeroU32>,
    pub write_qps: Option<NonZeroU32>,
    pub cachelib_attempt_zstd: Option<bool>,
    pub put_behaviour: Option<PutBehaviour>,
}

impl ArgDefaults for BlobstoreArgDefaults {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        let mut defaults = Vec::new();
        if let Some(read_qps) = self.read_qps {
            defaults.push(("blobstore_read_qps", read_qps.to_string()));
        }
        if let Some(write_qps) = self.write_qps {
            defaults.push(("blobstore_write_qps", write_qps.to_string()));
        }
        if let Some(cachelib_attempt_zstd) = self.cachelib_attempt_zstd {
            defaults.push((
                "blobstore_cachelib_attempt_zstd",
                cachelib_attempt_zstd.to_string(),
            ));
        }
        if let Some(put_behaviour) = self.put_behaviour {
            defaults.push((
                "blobstore_put_behaviour",
                <&'static str>::from(put_behaviour).to_string(),
            ));
        }

        defaults
    }
}

fn delay_distribution(mean: Option<f64>, stddev: Option<f64>) -> Result<Option<Normal<f64>>> {
    match (mean, stddev) {
        (Some(mean), Some(stddev)) => {
            let dist = Normal::new(mean, stddev)?;
            Ok(Some(dist))
        }
        _ => Ok(None),
    }
}

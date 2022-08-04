/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;
use std::ops::Deref;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use stats::prelude::*;

use crate::Blobstore;
use crate::BlobstoreBytes;
use crate::BlobstoreEnumerationData;
use crate::BlobstoreGetData;
use crate::BlobstoreIsPresent;
use crate::BlobstoreKeyParam;
use crate::BlobstoreKeySource;
use crate::BlobstorePutOps;
use crate::BlobstoreUnlinkOps;
use crate::OverwriteStatus;
use crate::PutBehaviour;

define_stats_struct! {
    CountedBlobstoreStats("mononoke.blobstore.{}", prefix: String),
    get: timeseries(Rate, Sum),
    get_ok: timeseries(Rate, Sum),
    get_err: timeseries(Rate, Sum),
    put: timeseries(Rate, Sum),
    put_ok: timeseries(Rate, Sum),
    put_err: timeseries(Rate, Sum),
    put_not_checked: timeseries(Rate, Sum),
    put_new: timeseries(Rate, Sum),
    put_overwrote: timeseries(Rate, Sum),
    put_prevented: timeseries(Rate, Sum),
    is_present: timeseries(Rate, Sum),
    is_present_ok: timeseries(Rate, Sum),
    is_present_err: timeseries(Rate, Sum),
    copy: timeseries(Rate, Sum),
    copy_ok: timeseries(Rate, Sum),
    copy_err: timeseries(Rate, Sum),
    unlink: timeseries(Rate, Sum),
    unlink_ok: timeseries(Rate, Sum),
    unlink_err: timeseries(Rate, Sum),
    enumerate: timeseries(Rate, Sum),
    enumerate_ok: timeseries(Rate, Sum),
    enumerate_err: timeseries(Rate, Sum),
}

#[derive(Debug)]
pub struct CountedBlobstore<T> {
    blobstore: T,
    stats: CountedBlobstoreStats,
}

impl<T: Display> Display for CountedBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CountedBlob<{}>", &self.blobstore)
    }
}

impl<T> CountedBlobstore<T> {
    pub fn new(name: String, blobstore: T) -> Self {
        Self {
            blobstore,
            stats: CountedBlobstoreStats::new(name),
        }
    }

    pub fn into_inner(self) -> T {
        self.blobstore
    }

    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for CountedBlobstore<T> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.stats.get.add_value(1);
        let res = self.blobstore.get(ctx, key).await;
        match res {
            Ok(_) => self.stats.get_ok.add_value(1),
            Err(_) => self.stats.get_err.add_value(1),
        }
        res
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.stats.put.add_value(1);
        let res = self.blobstore.put(ctx, key, value).await;
        match res {
            Ok(()) => self.stats.put_ok.add_value(1),
            Err(_) => self.stats.put_err.add_value(1),
        }
        res
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.stats.is_present.add_value(1);
        let res = self.blobstore.is_present(ctx, key).await;
        match res {
            Ok(_) => self.stats.is_present_ok.add_value(1),
            Err(_) => self.stats.is_present_err.add_value(1),
        }
        res
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        self.stats.copy.add_value(1);
        let res = self.blobstore.copy(ctx, old_key, new_key).await;
        match res {
            Ok(()) => self.stats.copy_ok.add_value(1),
            Err(_) => self.stats.copy_err.add_value(1),
        }
        res
    }
}

impl<T: BlobstorePutOps> CountedBlobstore<T> {
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        self.stats.put.add_value(1);
        let res = if let Some(put_behaviour) = put_behaviour {
            self.blobstore
                .put_explicit(ctx, key, value, put_behaviour)
                .await
        } else {
            self.blobstore.put_with_status(ctx, key, value).await
        };
        match res {
            Ok(status) => {
                self.stats.put_ok.add_value(1);
                match status {
                    OverwriteStatus::NotChecked => self.stats.put_not_checked.add_value(1),
                    OverwriteStatus::New => self.stats.put_new.add_value(1),
                    OverwriteStatus::Overwrote => self.stats.put_overwrote.add_value(1),
                    OverwriteStatus::Prevented => self.stats.put_prevented.add_value(1),
                };
            }
            Err(_) => self.stats.put_err.add_value(1),
        }
        res
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for CountedBlobstore<T> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, Some(put_behaviour)).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, None).await
    }
}

#[async_trait]
impl<T: BlobstoreUnlinkOps> BlobstoreUnlinkOps for CountedBlobstore<T> {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.stats.unlink.add_value(1);
        let res = self.blobstore.unlink(ctx, key).await;
        match res {
            Ok(()) => self.stats.unlink_ok.add_value(1),
            Err(_) => self.stats.unlink_err.add_value(1),
        }
        res
    }
}

#[async_trait]
impl<T: BlobstoreKeySource> BlobstoreKeySource for CountedBlobstore<T> {
    async fn enumerate<'a>(
        &'a self,
        ctx: &'a CoreContext,
        range: &'a BlobstoreKeyParam,
    ) -> Result<BlobstoreEnumerationData> {
        self.stats.enumerate.add_value(1);
        let res = self.blobstore.enumerate(ctx, range).await;
        match res {
            Ok(_) => self.stats.enumerate_ok.add_value(1),
            Err(_) => self.stats.enumerate_err.add_value(1),
        }
        res
    }
}

impl<T: Blobstore> Deref for CountedBlobstore<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_inner()
    }
}

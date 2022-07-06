/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

pub trait ThriftConvert: Sized {
    type Thrift;
    fn from_thrift(t: Self::Thrift) -> Result<Self>;
    fn into_thrift(self) -> Self::Thrift;
}

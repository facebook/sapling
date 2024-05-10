/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bytes::Bytes;

use crate::ThriftConvert;

impl ThriftConvert for i32 {
    const NAME: &'static str = "i32";
    type Thrift = i32;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t)
    }
    fn into_thrift(self) -> Self::Thrift {
        self
    }
}

impl ThriftConvert for i64 {
    const NAME: &'static str = "i64";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t)
    }
    fn into_thrift(self) -> Self::Thrift {
        self
    }
}

impl ThriftConvert for u32 {
    const NAME: &'static str = "u32";
    type Thrift = i32;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t as u32)
    }
    fn into_thrift(self) -> Self::Thrift {
        self as i32
    }
}

impl ThriftConvert for u64 {
    const NAME: &'static str = "u64";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t as u64)
    }
    fn into_thrift(self) -> Self::Thrift {
        self as i64
    }
}

impl ThriftConvert for String {
    const NAME: &'static str = "String";
    type Thrift = String;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t)
    }
    fn into_thrift(self) -> Self::Thrift {
        self
    }
}

impl<T: ThriftConvert> ThriftConvert for Vec<T> {
    const NAME: &'static str = "Vec";
    type Thrift = Vec<T::Thrift>;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.into_iter()
            .map(T::from_thrift)
            .collect::<Result<Vec<_>>>()
    }
    fn into_thrift(self) -> Self::Thrift {
        self.into_iter().map(T::into_thrift).collect()
    }
}

impl ThriftConvert for Bytes {
    const NAME: &'static str = "Bytes";
    type Thrift = Bytes;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(t)
    }
    fn into_thrift(self) -> Self::Thrift {
        self
    }
}

impl ThriftConvert for () {
    const NAME: &'static str = "()";
    type Thrift = ();

    fn from_thrift(_: Self::Thrift) -> Result<Self> {
        Ok(())
    }
    fn into_thrift(self) -> Self::Thrift {
        self
    }
}

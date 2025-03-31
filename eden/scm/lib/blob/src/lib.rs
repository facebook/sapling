/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Clone, Debug)]
pub enum ScmBlob {
    Bytes(minibytes::Bytes),
    #[cfg(fbcode_build)]
    IOBuf(iobuf::IOBufShared),
}

impl ScmBlob {
    pub fn to_bytes(&self) -> minibytes::Bytes {
        match self {
            Self::Bytes(bytes) => bytes.clone(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => minibytes::Bytes::from(Vec::<u8>::from(buf.clone())),
        }
    }

    pub fn into_bytes(self) -> minibytes::Bytes {
        match self {
            Self::Bytes(bytes) => bytes,
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => minibytes::Bytes::from(Vec::<u8>::from(buf)),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => buf.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bytes(bytes) => bytes.is_empty(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => buf.is_empty(),
        }
    }
}

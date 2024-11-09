/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod lz4;

pub use lz4::LZ4Error as Error;

pub use crate::lz4::compress;
pub use crate::lz4::compresshc;
pub use crate::lz4::decompress;
pub use crate::lz4::decompress_into;
pub use crate::lz4::decompress_size;
pub type Result<T> = std::result::Result<T, Error>;

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thrift_types::edenfs::FileAttributes;
use thrift_types::fbthrift::ThriftEnum;

pub fn all_attributes() -> &'static [&'static str] {
    FileAttributes::variants()
}

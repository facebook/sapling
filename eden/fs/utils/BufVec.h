/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/io/IOBuf.h>

namespace facebook {
namespace eden {

/**
 * Represents data that may come from a buffer or a file descriptor.
 *
 * EdenFS does not currently support splicing between the FUSE device
 * pipe and the backing files in the overlay, but there's an opportunity
 * to improve performance on large files by enabling FUSE_CAP_SPLICE_READ or
 * FUSE_CAP_SPLICE_WRITE.
 *
 * So pretend we have a type that corresponds roughly to libfuse's fuse_bufvec.
 */
using BufVec = std::unique_ptr<folly::IOBuf>;

} // namespace eden
} // namespace facebook

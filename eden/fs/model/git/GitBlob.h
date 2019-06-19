/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>

namespace folly {
class IOBuf;
}

namespace facebook {
namespace eden {

class Hash;
class Blob;

/**
 * Creates an Eden Blob from the serialized version of a Git blob object.
 * As such, the SHA-1 of the gitBlobObject should match the hash.
 */
std::unique_ptr<Blob> deserializeGitBlob(
    const Hash& hash,
    const folly::IOBuf* data);
} // namespace eden
} // namespace facebook

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

namespace folly {
class IOBuf;
}

namespace facebook::eden {

class ObjectId;
class Hash20;
class Blob;

/**
 * Creates an Eden Blob from the serialized version of a Git blob object.
 * As such, the SHA-1 of the gitBlobObject should match the hash.
 */
std::unique_ptr<Blob> deserializeGitBlob(
    const ObjectId& hash,
    const folly::IOBuf* data);

} // namespace facebook::eden

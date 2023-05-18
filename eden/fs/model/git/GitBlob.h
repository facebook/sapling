/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include "eden/fs/model/BlobFwd.h"

namespace folly {
class IOBuf;
}

namespace facebook::eden {

class ObjectId;

/**
 * Creates an Eden Blob from the serialized version of a Git blob object.
 * As such, the SHA-1 of the gitBlobObject should match the hash.
 */
BlobPtr deserializeGitBlob(const ObjectId& hash, const folly::IOBuf* data);

} // namespace facebook::eden

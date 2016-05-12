/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/String.h>

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
    folly::ByteRange gitBlobObject);
std::unique_ptr<Blob> deserializeGitBlob(
    const Hash& hash,
    std::string&& gitBlobObject);
}
}

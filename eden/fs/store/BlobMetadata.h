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

#include <cstdint>
#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

/**
 * A small struct containing both the size and the SHA-1 hash of
 * a Blob's contents.
 */
class BlobMetadata {
 public:
  BlobMetadata(Hash contentsHash, uint64_t fileLength)
      : sha1(contentsHash), size(fileLength) {}

  Hash sha1;
  uint64_t size;
};
}
}

/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/io/IOBuf.h>
#include <string>
#include "Hash.h"

namespace facebook {
namespace eden {

class Blob {
 public:
  Blob(const Hash& hash, folly::IOBuf&& contents)
      : hash_{hash},
        contents_{std::move(contents)},
        size_{contents_.computeChainDataLength()} {}

  Blob(const Hash& hash, const folly::IOBuf& contents)
      : hash_{hash},
        contents_{contents},
        size_{contents_.computeChainDataLength()} {}

  const Hash& getHash() const {
    return hash_;
  }

  const folly::IOBuf& getContents() const {
    return contents_;
  }

  size_t getSize() const {
    return size_;
  }

 private:
  const Hash hash_;
  const folly::IOBuf contents_;
  const size_t size_;
};
} // namespace eden
} // namespace facebook

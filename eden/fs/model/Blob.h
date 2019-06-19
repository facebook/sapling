/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

  /**
   * Convenience constructor for unit tests. Always copies the given
   * StringPiece.
   */
  Blob(const Hash& hash, folly::StringPiece contents)
      : hash_{hash},
        contents_{folly::IOBuf::COPY_BUFFER, contents.data(), contents.size()},
        size_{contents.size()} {}

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

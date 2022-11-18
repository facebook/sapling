/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/IOBuf.h>
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

class Blob {
 public:
  Blob(const ObjectId& hash, folly::IOBuf&& contents)
      : hash_{hash},
        contents_{std::move(contents)},
        size_{contents_.computeChainDataLength()} {}

  Blob(const ObjectId& hash, const folly::IOBuf& contents)
      : hash_{hash},
        contents_{contents},
        size_{contents_.computeChainDataLength()} {}

  /**
   * Convenience constructor for unit tests. Always copies the given
   * StringPiece.
   */
  Blob(const ObjectId& hash, folly::StringPiece contents)
      : hash_{hash},
        contents_{folly::IOBuf::COPY_BUFFER, contents.data(), contents.size()},
        size_{contents.size()} {}

  const ObjectId& getHash() const {
    return hash_;
  }

  const folly::IOBuf& getContents() const {
    return contents_;
  }

  const std::string asString() const {
    auto dataBuf = contents_.cloneCoalescedAsValue();
    return std::string{
        reinterpret_cast<const char*>(dataBuf.data()), dataBuf.length()};
  }

  size_t getSize() const {
    return size_;
  }

  size_t getSizeBytes() const {
    return size_;
  }

 private:
  const ObjectId hash_;
  const folly::IOBuf contents_;
  const size_t size_;
};

} // namespace facebook::eden

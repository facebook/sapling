/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/BufVec.h"

namespace facebook {
namespace eden {

BufVec::Buf::Buf(std::unique_ptr<folly::IOBuf> buf) : buf(std::move(buf)) {}

BufVec::BufVec(std::unique_ptr<folly::IOBuf> buf) {
  items_.emplace_back(std::make_shared<Buf>(std::move(buf)));
}

folly::fbvector<struct iovec> BufVec::getIov() const {
  folly::fbvector<struct iovec> vec;

  for (const auto& b : items_) {
    DCHECK(b->fd == -1) << "we don't support splicing yet";
    b->buf->appendToIov(&vec);
  }

  return vec;
}

size_t BufVec::size() const {
  size_t total = 0;
  for (const auto& b : items_) {
    total += b->buf->computeChainDataLength();
  }
  return total;
}

std::string BufVec::copyData() const {
  std::string rv;
  rv.reserve(size());
  for (const auto& b : items_) {
    DCHECK(b->fd == -1) << "we don't support splicing yet";
    const auto* buf = b->buf.get();
    do {
      rv.append(reinterpret_cast<const char*>(buf->data()), buf->length());
      buf = buf->next();
    } while (buf != b->buf.get());
  }
  return rv;
}

} // namespace eden
} // namespace facebook

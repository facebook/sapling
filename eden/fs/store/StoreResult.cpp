/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "StoreResult.h"

#include <folly/io/IOBuf.h>

using folly::IOBuf;

namespace {
void freeString(void* /* buffer */, void* userData) {
  auto str = static_cast<std::string*>(userData);
  delete str;
}
} // namespace

namespace facebook {
namespace eden {

IOBuf StoreResult::iobufWrapper() const {
  ensureValid();
  return IOBuf{IOBuf::WRAP_BUFFER, bytes()};
}

folly::IOBuf StoreResult::extractIOBuf() {
  ensureValid();

  // Unfortunately RocksDB returns data to us in a std::string.  This makes it
  // difficult for us to control the lifetime.  We end up having to allocate a
  // new std::string on the heap, just to control when it will free the
  // underlying data it points to.
  auto stringPtr = std::make_unique<std::string>(std::move(data_));
  // Extract the data and size before we pass stringPtr.release()
  // to the IOBuf constructor.  Arguments are evaluated in an arbitrary order,
  // and we need to make sure we do this before release() happens.
  auto data = const_cast<char*>(stringPtr->data());
  auto size = stringPtr->size();
  return IOBuf(
      IOBuf::TAKE_OWNERSHIP, data, size, freeString, stringPtr.release());
}

[[noreturn]] void StoreResult::throwInvalidError() const {
  // Maybe we should define our own more specific error type in the future
  throw std::domain_error("value not present in store");
}
} // namespace eden
} // namespace facebook

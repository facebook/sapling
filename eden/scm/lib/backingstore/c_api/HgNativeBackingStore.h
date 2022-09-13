/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <memory>

#include "eden/scm/lib/backingstore/c_api/RustBackingStore.h"

namespace folly {
class IOBuf;
} // namespace folly

namespace facebook::eden {

class HgNativeBackingStore {
 public:
  HgNativeBackingStore(
      folly::StringPiece repository,
      bool useEdenApi,
      bool useAuxData,
      bool allowRetries);

  std::unique_ptr<folly::IOBuf>
  getBlob(folly::ByteRange name, folly::ByteRange node, bool local);

  std::shared_ptr<RustFileAuxData> getBlobMetadata(
      folly::ByteRange node,
      bool local);

  void getBlobMetadataBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::shared_ptr<RustFileAuxData>)>&& resolve);

  /**
   * Imports a list of files from Rust contentstore. `names` and `nodes` are
   * required to have the same length, and both are combined to constitute a
   * query for one file.
   *
   * Whenever the requested file is read, `resolve` will be called with the
   * index of the request in the passed vectors, along with an unique pointer
   * pointing to the file content.
   *
   * If `local` is true, this method will only look requested file on disk.
   */
  void getBlobBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::unique_ptr<folly::IOBuf>)>&& resolve);

  void getTreeBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::shared_ptr<RustTree>)>&& resolve);

  std::shared_ptr<RustTree> getTree(folly::ByteRange node, bool local);

  void flush();

 private:
  std::unique_ptr<RustBackingStore, std::function<void(RustBackingStore*)>>
      store_;
};

} // namespace facebook::eden

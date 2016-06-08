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

#include <folly/Range.h>
#include <rocksdb/db.h>
#include <memory>
#include <string>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"

namespace facebook {
namespace eden {

class Hash;

/*
 * LocalStore stores objects (trees and blobs) locally on disk.
 *
 * This is a content-addressed store, so objects can be only retrieved using
 * their hash.
 *
 * The LocalStore is only a cache.  If an object is not found in the LocalStore
 * then it will need to be retrieved from the BackingStore.
 *
 * LocalStore uses RocksDB for the underlying storage.
 */
class LocalStore {
 public:
  explicit LocalStore(folly::StringPiece pathToRocksDb);

  std::unique_ptr<std::string> get(const Hash& id) const;
  std::unique_ptr<Tree> getTree(const Hash& id) const;
  std::unique_ptr<Blob> getBlob(const Hash& id) const;
  std::unique_ptr<Hash> getSha1ForBlob(const Hash& id) const;

  void putTree(const Hash& id, folly::ByteRange treeData) const;
  void putBlob(const Hash& id, folly::ByteRange blobData, const Hash& sha1)
      const;

 private:
  std::string _get(folly::ByteRange key) const;

  std::unique_ptr<rocksdb::DB> db_;
};
}
}

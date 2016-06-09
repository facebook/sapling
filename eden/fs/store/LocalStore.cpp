/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "LocalStore.h"

#include <folly/Format.h>
#include <folly/String.h>
#include <rocksdb/db.h>
#include <array>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitBlob.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/rocksdb/RocksDbUtil.h"
#include "eden/fs/rocksdb/RocksException.h"
#include "eden/fs/store/StoreResult.h"

using facebook::eden::Hash;
using folly::ByteRange;
using folly::StringPiece;
using rocksdb::ReadOptions;
using rocksdb::Slice;
using rocksdb::WriteBatch;
using rocksdb::WriteOptions;
using std::string;
using std::unique_ptr;

namespace {

/**
 * For a blob, we write an entry whose key is the same of the blob's with the
 * SHA1_KEY_SUFFIX suffix appended to it that maps to the SHA-1 of the blob's
 * contents.
 *
 * Note that we use a suffix that is a single byte so that the resulting key can
 * fit efficiently in an fbstring.
 */
const unsigned char ATTRIBUTE_SHA_1 = 's';

class Sha1Key {
 public:
  explicit Sha1Key(const Hash& id) {
    memcpy(key_.data(), id.getBytes().data(), Hash::RAW_SIZE);
    key_[Hash::RAW_SIZE] = ATTRIBUTE_SHA_1;
  }

  ByteRange bytes() const {
    return ByteRange(key_.data(), key_.size());
  }

  Slice slice() const {
    return Slice{reinterpret_cast<const char*>(key_.data()), key_.size()};
  }

 private:
  std::array<uint8_t, Hash::RAW_SIZE + 1> key_;
};

rocksdb::Slice _createSlice(folly::ByteRange bytes) {
  return Slice(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

}

namespace facebook {
namespace eden {

LocalStore::LocalStore(StringPiece pathToRocksDb)
    : db_(createRocksDb(pathToRocksDb)) {}

LocalStore::~LocalStore() {}

StoreResult LocalStore::get(const Hash& id) const {
  return _get(id.getBytes());
}

// TODO(mbolin): Currently, all objects in our RocksDB are Git objects. We
// probably want to namespace these by column family going forward, at which
// point we might want to have a GitLocalStore that delegates to an
// LocalStore so a vanilla LocalStore has no knowledge of deserializeGitTree()
// or deserializeGitBlob().

std::unique_ptr<Tree> LocalStore::getTree(const Hash& id) const {
  auto result = _get(id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  return deserializeGitTree(id, result.bytes());
}

std::unique_ptr<Blob> LocalStore::getBlob(const Hash& id) const {
  // We have to hold this string in scope while we deserialize and build
  // the blob; otherwise, the results are undefined.
  auto result = _get(id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  auto buf = result.extractIOBuf();
  return deserializeGitBlob(id, &buf);
}

std::unique_ptr<Hash> LocalStore::getSha1ForBlob(const Hash& id) const {
  Sha1Key key(id);
  auto result = _get(key.bytes());
  if (!result.isValid()) {
    return nullptr;
  }
  auto bytes = result.bytes();
  if (bytes.size() != Hash::RAW_SIZE) {
    throw std::invalid_argument(folly::sformat(
        "Database entry for {} was not of size {}. Could not convert to SHA-1.",
        id.toString(),
        static_cast<size_t>(Hash::RAW_SIZE)));
  }

  return std::make_unique<Hash>(bytes);
}

void LocalStore::putBlob(const Hash& id, ByteRange blobData, const Hash& sha1)
    const {
  Sha1Key sha1Key(id);

  // Record both the blob and SHA-1 entries in one RocksDB write operation.
  WriteBatch blobWrites;
  blobWrites.Put(_createSlice(id.getBytes()), _createSlice(blobData));
  blobWrites.Put(sha1Key.slice(), _createSlice(sha1.getBytes()));
  auto status = db_->Write(WriteOptions(), &blobWrites);
  if (!status.ok()) {
    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status,
        "error putting blob ",
        folly::hexlify(id.getBytes()),
        " in local store");
  }
}

void LocalStore::putTree(const Hash& id, ByteRange treeData) const {
  auto treeKey = _createSlice(id.getBytes());
  auto treeValue = _createSlice(treeData);
  auto status = db_->Put(WriteOptions(), treeKey, treeValue);
  if (!status.ok()) {
    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status,
        "error putting tree ",
        folly::hexlify(id.getBytes()),
        " in local store");
  }
}

StoreResult LocalStore::_get(ByteRange key) const {
  string value;
  auto status = db_.get()->Get(ReadOptions(), _createSlice(key), &value);
  if (!status.ok()) {
    if (status.IsNotFound()) {
      // Return an empty StoreResult
      return StoreResult();
    }

    // TODO: RocksDB can return a "TryAgain" error.
    // Should we try again for the user, rather than re-throwing the error?

    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status, "failed to get ", folly::hexlify(key), " from local store");
  }
  return StoreResult(std::move(value));
}
}
}

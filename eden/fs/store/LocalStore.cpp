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
#include <array>
#include "eden/fs/model/git/GitBlob.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/rocksdb/RocksDbUtil.h"
#include "eden/fs/rocksdb/RocksException.h"

using facebook::eden::Hash;
using folly::ByteRange;
using folly::StringPiece;
using rocksdb::DB;
using rocksdb::Options;
using rocksdb::ReadOptions;
using rocksdb::Slice;
using rocksdb::Status;
using rocksdb::WriteBatch;
using rocksdb::WriteOptions;
using std::array;
using std::string;
using std::unique_ptr;

/**
 * For a blob, we write an entry whose key is the same of the blob's with the
 * SHA1_KEY_SUFFIX suffix appended to it that maps to the SHA-1 of the blob's
 * contents.
 *
 * Note that we use a suffix that is a single byte so that the resulting key can
 * fit efficiently in an fbstring.
 */
const unsigned char ATTRIBUTE_SHA_1 = 's';

namespace {
rocksdb::Slice _createSliceForStringPiece(folly::StringPiece str) {
  return Slice(str.data(), str.size());
}

rocksdb::Slice _createSliceForByteRange(folly::ByteRange str) {
  return Slice(reinterpret_cast<const char*>(str.data()), str.size());
}

rocksdb::Slice _createSliceForHash(const Hash& hash) {
  return Slice(
      reinterpret_cast<const char*>(hash.getBytes().data()), Hash::RAW_SIZE);
}
}

namespace facebook {
namespace eden {

LocalStore::LocalStore(StringPiece pathToRocksDb)
    : db_(createRocksDb(pathToRocksDb)) {}

std::unique_ptr<string> LocalStore::get(const Hash& id) const {
  return std::make_unique<string>(_get(_createSliceForHash(id)));
}

// TODO(mbolin): Currently, all objects in our RocksDB are Git objects. We
// probably want to namespace these by column family going forward, at which
// point we might want to have a GitLocalStore that delegates to an
// LocalStore so a vanilla LocalStore has no knowledge of deserializeGitTree()
// or deserializeGitBlob().

std::unique_ptr<Tree> LocalStore::getTree(const Hash& id) const {
  auto gitTreeObject = get(id);
  return deserializeGitTree(id, folly::StringPiece{*gitTreeObject});
}

std::unique_ptr<Blob> LocalStore::getBlob(const Hash& id) const {
  // We have to hold this string in scope while we deserialize and build
  // the blob; otherwise, the results are undefined.
  std::unique_ptr<string> gitBlobObject = get(id);
  return deserializeGitBlob(id, std::move(*gitBlobObject));
}

std::unique_ptr<Hash> LocalStore::getSha1ForBlob(const Hash& id) const {
  auto keyAsFbstr = _getSha1KeyForHash(id);
  auto key = _createSliceForStringPiece(StringPiece{keyAsFbstr});
  string gitBlobObject = _get(key);
  if (gitBlobObject.size() != Hash::RAW_SIZE) {
    throw std::invalid_argument(folly::sformat(
        "Database entry for {} was not of size {}. Could not convert to SHA-1.",
        id.toString(),
        static_cast<size_t>(Hash::RAW_SIZE)));
  }

  ByteRange byteRange = StringPiece{gitBlobObject};
  return std::make_unique<Hash>(byteRange);
}

void LocalStore::putBlob(const Hash& id, ByteRange blobData, const Hash& sha1)
    const {
  auto blobKey = _createSliceForHash(id);
  auto blobValue = _createSliceForByteRange(blobData);

  auto keyAsFbstr = _getSha1KeyForHash(id);
  auto sha1Key = _createSliceForStringPiece(StringPiece{keyAsFbstr});
  auto sha1Value = _createSliceForHash(sha1);

  // Record both the blob and SHA-1 entries in one RocksDB write operation.
  WriteBatch blobWrites;
  blobWrites.Put(blobKey, blobValue);
  blobWrites.Put(sha1Key, sha1Value);
  auto status = db_->Write(WriteOptions(), &blobWrites);
  facebook::eden::RocksException::check(
      status, "put blob ", blobKey.ToString(/* hex */ true), " failed");
}

void LocalStore::putTree(const Hash& id, ByteRange treeData) const {
  auto treeKey = _createSliceForHash(id);
  auto treeValue = _createSliceForByteRange(treeData);
  auto status = db_->Put(WriteOptions(), treeKey, treeValue);
  facebook::eden::RocksException::check(
      status, "put tree ", treeKey.ToString(/* hex */ true), " failed");
}

folly::fbstring LocalStore::_getSha1KeyForHash(const Hash& id) const {
  auto keySize = Hash::RAW_SIZE + 1;
  char key[keySize];
  memcpy(key, id.getBytes().data(), Hash::RAW_SIZE);
  key[Hash::RAW_SIZE] = ATTRIBUTE_SHA_1;
  return folly::fbstring(key, keySize);
}

string LocalStore::_get(const char* key, size_t size) const {
  auto keyAsSlice = Slice(key, size);
  return _get(keyAsSlice);
}

string LocalStore::_get(Slice key) const {
  string value;
  auto status = db_.get()->Get(ReadOptions(), key, &value);
  RocksException::check(status, "get ", key.ToString(), " failed");
  return value;
}
}
}

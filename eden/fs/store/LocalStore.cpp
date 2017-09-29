/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "LocalStore.h"

#include <folly/Bits.h>
#include <folly/Format.h>
#include <folly/Optional.h>
#include <folly/String.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <rocksdb/db.h>
#include <rocksdb/filter_policy.h>
#include <rocksdb/table.h>
#include <array>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitBlob.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/rocksdb/RocksException.h"
#include "eden/fs/rocksdb/RocksHandles.h"
#include "eden/fs/store/StoreResult.h"

using facebook::eden::Hash;
using folly::ByteRange;
using folly::io::Cursor;
using folly::IOBuf;
using folly::Optional;
using folly::StringPiece;
using rocksdb::ReadOptions;
using rocksdb::Slice;
using rocksdb::SliceParts;
using rocksdb::WriteBatch;
using rocksdb::WriteOptions;
using std::string;
using std::unique_ptr;

namespace {
using namespace facebook::eden;

rocksdb::ColumnFamilyOptions makeColumnOptions(uint64_t LRUblockCacheSizeMB) {
  rocksdb::ColumnFamilyOptions options;

  // We'll never perform range scans on any of the keys that we store.
  // This enables bloom filters and a hash policy that improves our
  // get/put performance.
  options.OptimizeForPointLookup(LRUblockCacheSizeMB);

  options.OptimizeLevelStyleCompaction();
  return options;
}

/**
 * The different key spaces that we desire.
 * The ordering is coupled with the values of the LocalStore::KeySpace enum.
 */
const std::vector<rocksdb::ColumnFamilyDescriptor>& columnFamilies() {
  // Most of the column families will share the same cache.  We
  // want the blob data to live in its own smaller cache; the assumption
  // is that the vfs cache will compensate for that, together with the
  // idea that we shouldn't need to materialize a great many files.
  auto options = makeColumnOptions(64);
  auto blobOptions = makeColumnOptions(8);

  // Meyers singleton to avoid SIOF issues
  static const std::vector<rocksdb::ColumnFamilyDescriptor> families{
      rocksdb::ColumnFamilyDescriptor{rocksdb::kDefaultColumnFamilyName,
                                      options},
      rocksdb::ColumnFamilyDescriptor{"blob", blobOptions},
      rocksdb::ColumnFamilyDescriptor{"blobmeta", options},
      rocksdb::ColumnFamilyDescriptor{"tree", options},
      rocksdb::ColumnFamilyDescriptor{"hgproxyhash", options},
      rocksdb::ColumnFamilyDescriptor{"hgcommit2tree", options},
  };
  return families;
}

class SerializedBlobMetadata {
 public:
  explicit SerializedBlobMetadata(const BlobMetadata& metadata) {
    serialize(metadata.sha1, metadata.size);
  }
  SerializedBlobMetadata(const Hash& contentsHash, uint64_t blobSize) {
    serialize(contentsHash, blobSize);
  }

  Slice slice() const {
    return Slice{reinterpret_cast<const char*>(data_.data()), data_.size()};
  }

  static BlobMetadata parse(Hash blobID, const StoreResult& result) {
    auto bytes = result.bytes();
    if (bytes.size() != SIZE) {
      throw std::invalid_argument(folly::sformat(
          "Blob metadata for {} had unexpected size {}. Could not deserialize.",
          blobID.toString(),
          bytes.size()));
    }

    uint64_t blobSizeBE;
    memcpy(&blobSizeBE, bytes.data(), sizeof(uint64_t));
    bytes.advance(sizeof(uint64_t));
    auto contentsHash = Hash{bytes};
    return BlobMetadata{contentsHash, folly::Endian::big(blobSizeBE)};
  }

 private:
  void serialize(const Hash& contentsHash, uint64_t blobSize) {
    uint64_t blobSizeBE = folly::Endian::big(blobSize);
    memcpy(data_.data(), &blobSizeBE, sizeof(uint64_t));
    memcpy(
        data_.data() + sizeof(uint64_t),
        contentsHash.getBytes().data(),
        Hash::RAW_SIZE);
  }

  static constexpr size_t SIZE = sizeof(uint64_t) + Hash::RAW_SIZE;

  /**
   * The serialized data is stored as stored as:
   * - size (8 bytes, big endian)
   * - hash (20 bytes)
   */
  std::array<uint8_t, SIZE> data_;
};

rocksdb::Slice _createSlice(folly::ByteRange bytes) {
  return Slice(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}
}

namespace facebook {
namespace eden {

LocalStore::LocalStore(AbsolutePathPiece pathToRocksDb)
    : dbHandles_(pathToRocksDb.stringPiece(), columnFamilies()) {}

LocalStore::~LocalStore() {
#ifdef FOLLY_SANITIZE_ADDRESS
  // RocksDB has some race conditions around setting up and tearing down
  // the threads that it uses to maintain the database.  This manifests
  // in our test harness, particularly in a test where we quickly mount
  // and then unmount.  We see this as an abort with the message:
  // "pthread lock: Invalid Argument".
  // My assumption is that we're shutting things down before rocks has
  // completed initializing.  This sleep call is present in the destructor
  // to make it more likely that rocks is past that critical point and
  // so that we can shutdown successfully.
  /* sleep override */ sleep(1);
#endif
}

StoreResult LocalStore::get(KeySpace keySpace, ByteRange key) const {
  string value;
  auto status = dbHandles_.db.get()->Get(
      ReadOptions(),
      dbHandles_.columns[keySpace].get(),
      _createSlice(key),
      &value);
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

StoreResult LocalStore::get(KeySpace keySpace, const Hash& id) const {
  return get(keySpace, id.getBytes());
}

// TODO(mbolin): Currently, all objects in our RocksDB are Git objects. We
// probably want to namespace these by column family going forward, at which
// point we might want to have a GitLocalStore that delegates to an
// LocalStore so a vanilla LocalStore has no knowledge of deserializeGitTree()
// or deserializeGitBlob().

std::unique_ptr<Tree> LocalStore::getTree(const Hash& id) const {
  auto result = get(KeySpace::TreeFamily, id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  return deserializeGitTree(id, result.bytes());
}

std::unique_ptr<Blob> LocalStore::getBlob(const Hash& id) const {
  // We have to hold this string in scope while we deserialize and build
  // the blob; otherwise, the results are undefined.
  auto result = get(KeySpace::BlobFamily, id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  auto buf = result.extractIOBuf();
  return deserializeGitBlob(id, &buf);
}

Optional<BlobMetadata> LocalStore::getBlobMetadata(const Hash& id) const {
  auto result = get(KeySpace::BlobMetaDataFamily, id);
  if (!result.isValid()) {
    return folly::none;
  }
  return SerializedBlobMetadata::parse(id, result);
}

Optional<Hash> LocalStore::getSha1ForBlob(const Hash& id) const {
  auto metadata = getBlobMetadata(id);
  if (!metadata) {
    return folly::none;
  }
  return metadata.value().sha1;
}

std::pair<Hash, folly::IOBuf> LocalStore::serializeTree(const Tree* tree) {
  GitTreeSerializer serializer;
  for (auto& entry : tree->getTreeEntries()) {
    serializer.addEntry(std::move(entry));
  }
  IOBuf treeBuf = serializer.finalize();

  auto id = tree->getHash();
  if (id == Hash()) {
    id = Hash::sha1(&treeBuf);
  }
  return std::make_pair(id, treeBuf);
}

bool LocalStore::hasKey(KeySpace keySpace, folly::ByteRange key) const {
  string value;
  auto status = dbHandles_.db->Get(
      ReadOptions(),
      dbHandles_.columns[keySpace].get(),
      _createSlice(key),
      &value);
  if (!status.ok()) {
    if (status.IsNotFound()) {
      return false;
    }

    // TODO: RocksDB can return a "TryAgain" error.
    // Should we try again for the user, rather than re-throwing the error?

    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status, "failed to get ", folly::hexlify(key), " from local store");
  }
  return true;
}

bool LocalStore::hasKey(KeySpace keySpace, const Hash& id) const {
  return hasKey(keySpace, id.getBytes());
}

LocalStore::WriteBatch LocalStore::beginWrite(size_t bufSize) {
  return LocalStore::WriteBatch(dbHandles_, bufSize);
}

LocalStore::WriteBatch::WriteBatch(RocksHandles& dbHandles, size_t bufSize)
    : dbHandles_(dbHandles), writeBatch_(bufSize), bufSize_(bufSize) {}

LocalStore::WriteBatch::~WriteBatch() {
  if (writeBatch_.Count() > 0) {
    XLOG(ERR) << "WriteBatch being destroyed with " << writeBatch_.Count()
              << " items pending flush";
  }
}

Hash LocalStore::putTree(const Tree* tree) {
  auto serialized = LocalStore::serializeTree(tree);
  ByteRange treeData = serialized.second.coalesce();

  auto& id = serialized.first;
  put(KeySpace::TreeFamily, id, treeData);
  return id;
}

Hash LocalStore::WriteBatch::putTree(const Tree* tree) {
  auto serialized = LocalStore::serializeTree(tree);
  ByteRange treeData = serialized.second.coalesce();

  auto& id = serialized.first;
  put(KeySpace::TreeFamily, id.getBytes(), treeData);
  return id;
}

BlobMetadata LocalStore::putBlob(const Hash& id, const Blob* blob) {
  // Since blob serialization is moderately complex, just delegate
  // the immediate putBlob to the method on the WriteBatch.
  // Pre-allocate a buffer of approximately the right size; it
  // needs to hold the blob content plus have room for a couple of
  // hashes for the keys, plus some padding.
  auto batch = beginWrite(blob->getContents().computeChainDataLength() + 64);
  auto result = batch.putBlob(id, blob);
  batch.flush();
  return result;
}

BlobMetadata LocalStore::WriteBatch::putBlob(const Hash& id, const Blob* blob) {
  const IOBuf& contents = blob->getContents();

  BlobMetadata metadata{Hash::sha1(&contents),
                        contents.computeChainDataLength()};

  SerializedBlobMetadata metadataBytes(metadata);

  auto hashSlice = _createSlice(id.getBytes());
  SliceParts keyParts(&hashSlice, 1);

  ByteRange bodyBytes;

  // Add a git-style blob prefix
  auto prefix = folly::to<string>("blob ", contents.computeChainDataLength());
  prefix.push_back('\0');
  std::vector<Slice> bodySlices;
  bodySlices.emplace_back(prefix);

  // Add all of the IOBuf chunks
  Cursor cursor(&contents);
  while (true) {
    auto bytes = cursor.peekBytes();
    if (bytes.empty()) {
      break;
    }
    bodySlices.push_back(_createSlice(bytes));
    cursor.skip(bytes.size());
  }

  SliceParts bodyParts(bodySlices.data(), bodySlices.size());

  writeBatch_.Put(
      dbHandles_.columns[KeySpace::BlobFamily].get(), keyParts, bodyParts);

  writeBatch_.Put(
      dbHandles_.columns[KeySpace::BlobMetaDataFamily].get(),
      hashSlice,
      metadataBytes.slice());
  flushIfNeeded();
  return metadata;
}

void LocalStore::WriteBatch::flush() {
  auto pending = writeBatch_.Count();
  if (pending == 0) {
    return;
  }

  XLOG(DBG5) << "Flushing " << pending << " entries with data size of "
             << writeBatch_.GetDataSize();

  auto status = dbHandles_.db->Write(WriteOptions(), &writeBatch_);
  XLOG(DBG5) << "... Flushed";

  if (!status.ok()) {
    throw RocksException::build(
        status, "error putting blob batch in local store");
  }

  writeBatch_.Clear();
}

void LocalStore::WriteBatch::flushIfNeeded() {
  auto needFlush = bufSize_ > 0 && writeBatch_.GetDataSize() >= bufSize_;

  if (needFlush) {
    flush();
  }
}

void LocalStore::put(
    LocalStore::KeySpace keySpace,
    const Hash& id,
    folly::ByteRange value) {
  put(keySpace, id.getBytes(), value);
}

void LocalStore::put(
    LocalStore::KeySpace keySpace,
    folly::ByteRange key,
    folly::ByteRange value) {
  dbHandles_.db->Put(
      WriteOptions(),
      dbHandles_.columns[keySpace].get(),
      _createSlice(key),
      _createSlice(value));
}

void LocalStore::WriteBatch::put(
    LocalStore::KeySpace keySpace,
    const Hash& id,
    folly::ByteRange value) {
  put(keySpace, id.getBytes(), value);
}

void LocalStore::WriteBatch::put(
    LocalStore::KeySpace keySpace,
    folly::ByteRange key,
    folly::ByteRange value) {
  writeBatch_.Put(
      dbHandles_.columns[keySpace].get(),
      _createSlice(key),
      _createSlice(value));

  flushIfNeeded();
}

} // namespace eden
} // namespace facebook

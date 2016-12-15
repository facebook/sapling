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

#include <folly/Bits.h>
#include <folly/Format.h>
#include <folly/Optional.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
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

/**
 * For each blob, we also store an entry containing the blob metadata.
 * This is stored under a key that is the blob's key plus the
 * ATTRIBUTE_METADATA suffix.
 *
 * (We should potentially switch to RocksDB column families instead in the
 * future, rather than using a key suffix.)
 */
const unsigned char ATTRIBUTE_METADATA = 'x';

class BlobMetadataKey {
 public:
  explicit BlobMetadataKey(const Hash& id) {
    memcpy(key_.data(), id.getBytes().data(), Hash::RAW_SIZE);
    key_[Hash::RAW_SIZE] = ATTRIBUTE_METADATA;
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
    : db_(createRocksDb(pathToRocksDb.stringPiece())) {}

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

StoreResult LocalStore::get(ByteRange key) const {
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

StoreResult LocalStore::get(const Hash& id) const {
  return get(id.getBytes());
}

// TODO(mbolin): Currently, all objects in our RocksDB are Git objects. We
// probably want to namespace these by column family going forward, at which
// point we might want to have a GitLocalStore that delegates to an
// LocalStore so a vanilla LocalStore has no knowledge of deserializeGitTree()
// or deserializeGitBlob().

std::unique_ptr<Tree> LocalStore::getTree(const Hash& id) const {
  auto result = get(id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  return deserializeGitTree(id, result.bytes());
}

std::unique_ptr<Blob> LocalStore::getBlob(const Hash& id) const {
  // We have to hold this string in scope while we deserialize and build
  // the blob; otherwise, the results are undefined.
  auto result = get(id.getBytes());
  if (!result.isValid()) {
    return nullptr;
  }
  auto buf = result.extractIOBuf();
  return deserializeGitBlob(id, &buf);
}

Optional<BlobMetadata> LocalStore::getBlobMetadata(const Hash& id) const {
  BlobMetadataKey key(id);
  auto result = get(key.bytes());
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

BlobMetadata LocalStore::putBlob(const Hash& id, const Blob* blob) {
  const IOBuf& contents = blob->getContents();

  BlobMetadata metadata{Hash::sha1(&contents),
                        contents.computeChainDataLength()};
  BlobMetadataKey metadataKey(id);
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

  WriteBatch blobWrites;
  blobWrites.Put(keyParts, bodyParts);
  blobWrites.Put(metadataKey.slice(), metadataBytes.slice());
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

  return metadata;
}

Hash LocalStore::putTree(const Tree* tree) {
  GitTreeSerializer serializer;
  for (auto& entry : tree->getTreeEntries()) {
    serializer.addEntry(std::move(entry));
  }
  IOBuf treeBuf = serializer.finalize();
  ByteRange treeData = treeBuf.coalesce();

  auto id = tree->getHash();
  if (id == Hash()) {
    id = Hash::sha1(&treeBuf);
  }
  put(id.getBytes(), treeData);
  return id;
}

void LocalStore::put(folly::ByteRange key, folly::ByteRange value) {
  auto status =
      db_->Put(WriteOptions(), _createSlice(key), _createSlice(value));
  if (!status.ok()) {
    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status,
        "error putting data for key ",
        folly::hexlify(key),
        " in local store");
  }
}

}
}

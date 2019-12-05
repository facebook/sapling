/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/LocalStore.h"

#include <folly/Format.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>
#include <array>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitBlob.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/store/KeySpaces.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Cursor;
using std::optional;
using std::string;

namespace facebook {
namespace eden {

LocalStore::LocalStore() noexcept {}

void LocalStore::clearCachesAndCompactAll() {
  for (auto ks : kKeySpaceRecords) {
    if (ks.persistence == Persistence::Ephemeral) {
      clearKeySpace(ks.keySpace);
    }
    compactKeySpace(ks.keySpace);
  }
}

void LocalStore::clearCaches() {
  for (auto ks : kKeySpaceRecords) {
    if (ks.persistence == Persistence::Ephemeral) {
      clearKeySpace(ks.keySpace);
    }
  }
}

void LocalStore::compactStorage() {
  for (auto ks : kKeySpaceRecords) {
    compactKeySpace(ks.keySpace);
  }
}

StoreResult LocalStore::get(KeySpace keySpace, const Hash& id) const {
  return get(keySpace, id.getBytes());
}

// This is the fallback implementation for stores that don't have any
// internal support for asynchronous fetches.  This just performs the
// fetch and wraps it in a future
folly::Future<StoreResult> LocalStore::getFuture(
    KeySpace keySpace,
    folly::ByteRange key) const {
  return folly::makeFutureWith(
      [keySpace, key, this] { return get(keySpace, key); });
}

folly::Future<std::vector<StoreResult>> LocalStore::getBatch(
    KeySpace keySpace,
    const std::vector<folly::ByteRange>& keys) const {
  return folly::makeFutureWith([keySpace, keys, this] {
    std::vector<StoreResult> results;
    for (auto& key : keys) {
      results.emplace_back(get(keySpace, key));
    }
    return results;
  });
}

// TODO(mbolin): Currently, all objects in our RocksDB are Git objects. We
// probably want to namespace these by column family going forward, at which
// point we might want to have a GitLocalStore that delegates to an
// LocalStore so a vanilla LocalStore has no knowledge of deserializeGitTree()
// or deserializeGitBlob().

folly::Future<std::unique_ptr<Tree>> LocalStore::getTree(const Hash& id) const {
  return getFuture(KeySpace::TreeFamily, id.getBytes())
      .thenValue([id](StoreResult&& data) {
        if (!data.isValid()) {
          return std::unique_ptr<Tree>(nullptr);
        }
        return deserializeGitTree(id, data.bytes());
      });
}

folly::Future<std::unique_ptr<Blob>> LocalStore::getBlob(const Hash& id) const {
  return getFuture(KeySpace::BlobFamily, id.getBytes())
      .thenValue([id](StoreResult&& data) {
        if (!data.isValid()) {
          return std::unique_ptr<Blob>(nullptr);
        }
        auto buf = data.extractIOBuf();
        return deserializeGitBlob(id, &buf);
      });
}

folly::Future<optional<BlobMetadata>> LocalStore::getBlobMetadata(
    const Hash& id) const {
  return getFuture(KeySpace::BlobMetaDataFamily, id.getBytes())
      .thenValue([id](StoreResult&& data) -> optional<BlobMetadata> {
        if (!data.isValid()) {
          return std::nullopt;
        } else {
          return SerializedBlobMetadata::parse(id, data);
        }
      });
}

folly::Future<std::optional<uint64_t>> LocalStore::getBlobSize(
    const Hash& id) const {
  return getFuture(KeySpace::BlobSizeFamily, id.getBytes())
      .thenValue([](StoreResult&& data) -> std::optional<uint64_t> {
        if (!data.isValid()) {
          return std::nullopt;
        }

        folly::IOBuf dataIOBuf = data.iobufWrapper();
        folly::io::Cursor cursor{&dataIOBuf};
        return cursor.readBE<uint64_t>();
      });
}

std::pair<Hash, folly::IOBuf> LocalStore::serializeTree(const Tree* tree) {
  GitTreeSerializer serializer;
  for (auto& entry : tree->getTreeEntries()) {
    serializer.addEntry(std::move(entry));
  }
  IOBuf treeBuf = serializer.finalize();

  auto id = tree->getHash();
  if (id == Hash()) {
    id = Hash::sha1(treeBuf);
  }
  return std::make_pair(id, treeBuf);
}

bool LocalStore::hasKey(KeySpace keySpace, const Hash& id) const {
  return hasKey(keySpace, id.getBytes());
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
  BlobMetadata metadata = getMetadataFromBlob(blob);

  putBlobWithoutMetadata(id, blob);
  putBlobMetadata(id, metadata);

  return metadata;
}

BlobMetadata LocalStore::getMetadataFromBlob(const Blob* blob) {
  Hash sha1 = Hash::sha1(blob->getContents());
  uint64_t size = blob->getSize();
  return BlobMetadata{sha1, size};
}

void LocalStore::putBlobWithoutMetadata(const Hash& id, const Blob* blob) {
  if (!enableBlobCaching) {
    XLOG(DBG8) << "Skipping caching " << id
               << " because blob cache is disabled via config";
    return;
  }
  // Since blob serialization is moderately complex, just delegate
  // the immediate putBlob to the method on the WriteBatch.
  // Pre-allocate a buffer of approximately the right size; it
  // needs to hold the blob content plus have room for a couple of
  // hashes for the keys, plus some padding.
  auto batch = beginWrite(blob->getSize() + 64);
  batch->putBlob(id, blob);
  batch->flush();
}

void LocalStore::putBlobMetadata(const Hash& id, const BlobMetadata& metadata) {
  auto hashBytes = id.getBytes();
  SerializedBlobMetadata metadataBytes(metadata);

  put(LocalStore::KeySpace::BlobMetaDataFamily,
      hashBytes,
      metadataBytes.slice());

  putBlobSize(id, metadata.size);
}

void LocalStore::putBlobSize(const Hash& id, const uint64_t size) {
  auto hashBytes = id.getBytes();

  uint64_t sizeBE = folly::Endian::big(size);
  std::array<uint8_t, sizeof(uint64_t)> bytes;
  memcpy(bytes.data(), &sizeBE, sizeof(uint64_t));
  auto sizeBytes = folly::ByteRange{bytes};

  put(LocalStore::KeySpace::BlobSizeFamily, hashBytes, sizeBytes);
}

void LocalStore::put(
    LocalStore::KeySpace keySpace,
    const Hash& id,
    folly::ByteRange value) {
  put(keySpace, id.getBytes(), value);
}

void LocalStore::WriteBatch::put(
    LocalStore::KeySpace keySpace,
    const Hash& id,
    folly::ByteRange value) {
  put(keySpace, id.getBytes(), value);
}

void LocalStore::WriteBatch::putBlob(const Hash& id, const Blob* blob) {
  const IOBuf& contents = blob->getContents();
  auto hashSlice = id.getBytes();

  // Add a git-style blob prefix
  auto prefix = folly::to<string>("blob ", blob->getSize());
  prefix.push_back('\0');
  std::vector<ByteRange> bodySlices;
  bodySlices.emplace_back(StringPiece(prefix));

  // Add all of the IOBuf chunks
  Cursor cursor(&contents);
  while (true) {
    auto bytes = cursor.peekBytes();
    if (bytes.empty()) {
      break;
    }
    bodySlices.push_back(bytes);
    cursor.skip(bytes.size());
  }

  put(LocalStore::KeySpace::BlobFamily, hashSlice, bodySlices);
}

LocalStore::WriteBatch::~WriteBatch() {}
LocalStore::~LocalStore() {}

void LocalStore::periodicManagementTask(const EdenConfig& /* config */) {
  // Individual store subclasses can provide their own implementations for
  // periodic management.
}

} // namespace eden
} // namespace facebook

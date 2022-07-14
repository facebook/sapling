/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/LocalStore.h"

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
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/TreeMetadata.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Cursor;
using std::optional;
using std::string;

namespace facebook::eden {

void LocalStore::clearDeprecatedKeySpaces() {
  for (auto& ks : KeySpace::kAll) {
    if (ks->isDeprecated()) {
      clearKeySpace(ks);
      compactKeySpace(ks);
    }
  }
}

void LocalStore::clearCachesAndCompactAll() {
  for (auto& ks : KeySpace::kAll) {
    if (ks->isEphemeral()) {
      clearKeySpace(ks);
    }
    compactKeySpace(ks);
  }
}

void LocalStore::clearCaches() {
  for (auto& ks : KeySpace::kAll) {
    if (ks->isEphemeral()) {
      clearKeySpace(ks);
    }
  }
}

void LocalStore::compactStorage() {
  for (auto& ks : KeySpace::kAll) {
    compactKeySpace(ks);
  }
}

StoreResult LocalStore::get(KeySpace keySpace, const ObjectId& id) const {
  return get(keySpace, id.getBytes());
}

// This is the fallback implementation for stores that don't have any
// internal support for asynchronous fetches.  This just performs the
// fetch and wraps it in a future
ImmediateFuture<StoreResult> LocalStore::getImmediateFuture(
    KeySpace keySpace,
    const ObjectId& id) const {
  return makeImmediateFutureWith([&] { return get(keySpace, id); });
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

ImmediateFuture<std::unique_ptr<Tree>> LocalStore::getTree(
    const ObjectId& id) const {
  return getImmediateFuture(KeySpace::TreeFamily, id)
      .thenValue([id](StoreResult&& data) {
        if (!data.isValid()) {
          return std::unique_ptr<Tree>(nullptr);
        }
        auto tree = Tree::tryDeserialize(id, StringPiece{data.bytes()});
        if (tree) {
          return tree;
        }
        return deserializeGitTree(id, data.bytes());
      });
}

ImmediateFuture<std::unique_ptr<Blob>> LocalStore::getBlob(
    const ObjectId& id) const {
  if (!enableBlobCaching) {
    return std::unique_ptr<Blob>(nullptr);
  }

  return getImmediateFuture(KeySpace::BlobFamily, id)
      .thenValue([id](StoreResult&& data) {
        if (!data.isValid()) {
          return std::unique_ptr<Blob>(nullptr);
        }
        auto buf = data.extractIOBuf();
        return deserializeGitBlob(id, &buf);
      });
}

ImmediateFuture<optional<BlobMetadata>> LocalStore::getBlobMetadata(
    const ObjectId& id) const {
  return getImmediateFuture(KeySpace::BlobMetaDataFamily, id)
      .thenValue([id](StoreResult&& data) -> optional<BlobMetadata> {
        if (!data.isValid()) {
          return std::nullopt;
        } else {
          return SerializedBlobMetadata::parse(id, data);
        }
      });
}

folly::IOBuf LocalStore::serializeTree(const Tree& tree) {
  return tree.serialize();
}

bool LocalStore::hasKey(KeySpace keySpace, const ObjectId& id) const {
  return hasKey(keySpace, id.getBytes());
}

void LocalStore::putTree(const Tree& tree) {
  auto serialized = LocalStore::serializeTree(tree);
  ByteRange treeData = serialized.coalesce();

  put(KeySpace::TreeFamily, tree.getHash().getBytes(), treeData);
}

void LocalStore::WriteBatch::putTree(const Tree& tree) {
  auto serialized = LocalStore::serializeTree(tree);
  ByteRange treeData = serialized.coalesce();

  put(KeySpace::TreeFamily, tree.getHash().getBytes(), treeData);
}

void LocalStore::putBlob(const ObjectId& id, const Blob* blob) {
  if (!enableBlobCaching) {
    XLOG(DBG8) << "Skipping caching " << id
               << " because blob cache is disabled via config";
  } else {
    // Since blob serialization is moderately complex, just delegate
    // the immediate putBlob to the method on the WriteBatch.
    // Pre-allocate a buffer of approximately the right size; it
    // needs to hold the blob content plus have room for a couple of
    // hashes for the keys, plus some padding.
    auto batch = beginWrite(blob->getSize() + 64);
    batch->putBlob(id, blob);
    batch->flush();
  }
}

BlobMetadata LocalStore::putBlobMetadata(const ObjectId& id, const Blob* blob) {
  BlobMetadata metadata{Hash20::sha1(blob->getContents()), blob->getSize()};
  auto hashBytes = id.getBytes();
  SerializedBlobMetadata metadataBytes(metadata);

  put(KeySpace::BlobMetaDataFamily, hashBytes, metadataBytes.slice());

  return metadata;
}

void LocalStore::put(
    KeySpace keySpace,
    const ObjectId& id,
    folly::ByteRange value) {
  XCHECK(!keySpace->isDeprecated())
      << "Write to deprecated keyspace " << keySpace->name;
  put(keySpace, id.getBytes(), value);
}

void LocalStore::WriteBatch::put(
    KeySpace keySpace,
    const ObjectId& id,
    folly::ByteRange value) {
  XCHECK(!keySpace->isDeprecated())
      << "Write to deprecated keyspace " << keySpace->name;
  put(keySpace, id.getBytes(), value);
}

void LocalStore::WriteBatch::putBlob(const ObjectId& id, const Blob* blob) {
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

  put(KeySpace::BlobFamily, hashSlice, bodySlices);
}

LocalStore::WriteBatch::~WriteBatch() {}

void LocalStore::periodicManagementTask(const EdenConfig& /* config */) {
  // Individual store subclasses can provide their own implementations for
  // periodic management.
}

} // namespace facebook::eden

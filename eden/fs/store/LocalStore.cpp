/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/LocalStore.h"

#include <folly/ExceptionWrapper.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <array>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitBlob.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/store/SerializedBlobAuxData.h"
#include "eden/fs/store/SerializedTreeAuxData.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/telemetry/EdenStats.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Cursor;
using std::optional;
using std::string;

namespace facebook::eden {

namespace {
template <typename T, typename F>
FOLLY_ALWAYS_INLINE std::shared_ptr<T> parse(
    const ObjectId& id,
    std::string_view context,
    const EdenStatsPtr& stats,
    StatsGroupBase::Counter LocalStoreStats::*successCounter,
    StatsGroupBase::Counter LocalStoreStats::*errorCounter,
    F&& fn) {
  std::shared_ptr<T> def(nullptr);
  if (auto ew = folly::try_and_catch(
          [&def, fn = std::forward<F>(fn)]() { def = fn(); })) {
    stats->increment(errorCounter);
    XLOGF(ERR, "Failed to get {} for {}: {}", context, id, ew.what());
  }

  stats->increment(successCounter);
  return def;
}
} // namespace

LocalStore::LocalStore(EdenStatsPtr edenStats) : stats_{std::move(edenStats)} {}

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

ImmediateFuture<std::vector<StoreResult>> LocalStore::getBatch(
    KeySpace keySpace,
    const std::vector<folly::ByteRange>& keys) const {
  return makeImmediateFutureWith([keySpace, keys, this] {
    std::vector<StoreResult> results;
    for (auto& key : keys) {
      results.emplace_back(get(keySpace, key));
    }
    return results;
  });
}

ImmediateFuture<TreePtr> LocalStore::getTree(const ObjectId& id) const {
  DurationScope<EdenStats> stat{stats_, &LocalStoreStats::getTree};
  return getImmediateFuture(KeySpace::TreeFamily, id)
      .thenValue(
          [id, stat = std::move(stat), stats = stats_.copy()](
              StoreResult&& data) -> TreePtr {
            if (data.isValid()) {
              return parse<const Tree>(
                  id,
                  "Tree",
                  stats,
                  &LocalStoreStats::getTreeSuccess,
                  &LocalStoreStats::getTreeError,
                  [&id, &data]() {
                    auto tree =
                        Tree::tryDeserialize(id, StringPiece{data.bytes()});
                    if (tree) {
                      return tree;
                    }

                    return deserializeGitTree(id, data.bytes());
                  });
            }

            stats->increment(&LocalStoreStats::getTreeFailure);
            return nullptr;
          });
}

ImmediateFuture<BlobPtr> LocalStore::getBlob(const ObjectId& id) const {
  DurationScope<EdenStats> stat{stats_, &LocalStoreStats::getBlob};
  return getImmediateFuture(KeySpace::BlobFamily, id)
      .thenValue(
          [id, stat = std::move(stat), stats = stats_.copy()](
              StoreResult&& data) -> BlobPtr {
            if (data.isValid()) {
              return parse<const Blob>(
                  id,
                  "Blob",
                  stats,
                  &LocalStoreStats::getBlobSuccess,
                  &LocalStoreStats::getBlobError,
                  [&data]() {
                    auto buf = data.extractIOBuf();
                    return deserializeGitBlob(&buf);
                  });
            }

            stats->increment(&LocalStoreStats::getBlobFailure);
            return nullptr;
          });
}

ImmediateFuture<BlobAuxDataPtr> LocalStore::getBlobAuxData(
    const ObjectId& id) const {
  DurationScope<EdenStats> stat{stats_, &LocalStoreStats::getBlobAuxData};
  return getImmediateFuture(KeySpace::BlobAuxDataFamily, id)
      .thenValue(
          [id, stat = std::move(stat), stats = stats_.copy()](
              StoreResult&& data) -> BlobAuxDataPtr {
            if (data.isValid()) {
              return parse<const BlobAuxData>(
                  id,
                  "BlobAuxData",
                  stats,
                  &LocalStoreStats::getBlobAuxDataSuccess,
                  &LocalStoreStats::getBlobAuxDataError,
                  [&id, &data]() {
                    return SerializedBlobAuxData::parse(id, data);
                  });
            }

            stats->increment(&LocalStoreStats::getBlobAuxDataFailure);
            return nullptr;
          });
}

ImmediateFuture<TreeAuxDataPtr> LocalStore::getTreeAuxData(
    const ObjectId& id) const {
  DurationScope<EdenStats> stat{stats_, &LocalStoreStats::getTreeAuxData};
  return getImmediateFuture(KeySpace::TreeAuxDataFamily, id)
      .thenValue(
          [id, stat = std::move(stat), stats = stats_.copy()](
              StoreResult&& data) -> TreeAuxDataPtr {
            if (data.isValid()) {
              return parse<const TreeAuxData>(
                  id,
                  "TreeAuxData",
                  stats,
                  &LocalStoreStats::getTreeAuxDataSuccess,
                  &LocalStoreStats::getTreeAuxDataError,
                  [&id, &data]() {
                    return SerializedTreeAuxData::parse(id, data);
                  });
            }

            stats->increment(&LocalStoreStats::getTreeAuxDataFailure);
            return nullptr;
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

  put(KeySpace::TreeFamily, tree.getObjectId().getBytes(), treeData);
}

void LocalStore::WriteBatch::putTree(const Tree& tree) {
  auto serialized = LocalStore::serializeTree(tree);
  ByteRange treeData = serialized.coalesce();

  put(KeySpace::TreeFamily, tree.getObjectId().getBytes(), treeData);
}

void LocalStore::putBlob(const ObjectId& id, const Blob* blob) {
  // Since blob serialization is moderately complex, just delegate
  // the immediate putBlob to the method on the WriteBatch.
  // Pre-allocate a buffer of approximately the right size; it
  // needs to hold the blob content plus have room for a couple of
  // ids for the keys, plus some padding.
  auto batch = beginWrite(blob->getSize() + 64);
  batch->putBlob(id, blob);
  batch->flush();
}

void LocalStore::putBlobAuxData(
    const ObjectId& id,
    const BlobAuxData& auxData) {
  auto idBytes = id.getBytes();
  SerializedBlobAuxData auxDataBytes(auxData);

  put(KeySpace::BlobAuxDataFamily, idBytes, auxDataBytes.slice());
}

void LocalStore::WriteBatch::putBlobAuxData(
    const ObjectId& id,
    const BlobAuxData& auxData) {
  auto idBytes = id.getBytes();
  SerializedBlobAuxData auxDataBytes(auxData);

  put(KeySpace::BlobAuxDataFamily, idBytes, auxDataBytes.slice());
}

void LocalStore::putTreeAuxData(
    const ObjectId& id,
    const TreeAuxData& auxData) {
  auto idBytes = id.getBytes();
  SerializedTreeAuxData auxDataBytes(auxData);

  put(KeySpace::TreeAuxDataFamily, idBytes, auxDataBytes.slice());
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
  auto idSlice = id.getBytes();

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

  put(KeySpace::BlobFamily, idSlice, bodySlices);
}

LocalStore::WriteBatch::~WriteBatch() = default;

void LocalStore::periodicManagementTask(const EdenConfig& /* config */) {
  // Individual store subclasses can provide their own implementations for
  // periodic management.
}

} // namespace facebook::eden

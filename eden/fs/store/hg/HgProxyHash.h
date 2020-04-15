/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/FixedString.h>
#include <string>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
class IOBuf;
} // namespace folly

namespace facebook {
namespace eden {
namespace {
// An empty ProxyHash. It contains an all zeros hash and zero length path.
constexpr auto kDefaultProxyHash =
    folly::FixedString<Hash::RAW_SIZE + sizeof(uint32_t)>().append(
        Hash::RAW_SIZE + sizeof(uint32_t),
        '\x00');
} // namespace

/**
 * HgProxyHash manages mercurial (path, revHash) data in the LocalStore.
 *
 * Mercurial doesn't really have a blob hash the same way eden and git do.
 * Instead, mercurial file revision hashes are always relative to a specific
 * path.  To use the data in eden, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the
 * blob hash in eden.  We store the eden_blob_hash --> (path, hgRevHash)
 * mapping in the LocalStore.  The HgProxyHash class helps store and
 * retrieve these mappings.
 */
class HgProxyHash {
 public:
  /**
   * Load HgProxyHash data for the given eden blob hash from the LocalStore.
   */
  HgProxyHash(LocalStore* store, Hash edenBlobHash, folly::StringPiece context);

  ~HgProxyHash() = default;

  HgProxyHash(const HgProxyHash& other) = default;
  HgProxyHash& operator=(const HgProxyHash& other) = default;

  HgProxyHash(HgProxyHash&& other) noexcept(false) {
    value_.swap(other.value_);
  }

  HgProxyHash& operator=(HgProxyHash&& other) noexcept(false) {
    value_.swap(other.value_);
    other.value_ = kDefaultProxyHash;
    return *this;
  }

  RelativePathPiece path() const;

  Hash revHash() const;

  static folly::Future<std::vector<HgProxyHash>> getBatch(
      LocalStore* store,
      const std::vector<Hash>& blobHashes);

  /**
   * Store HgProxyHash data in the LocalStore.
   *
   * Returns an eden blob hash that can be used to retrieve the data later
   * (using the HgProxyHash constructor defined above).
   */
  static Hash store(
      RelativePathPiece path,
      Hash hgRevHash,
      LocalStore::WriteBatch* writeBatch);

  /**
   * Compute the proxy hash information, but do not store it.
   *
   * This is useful when you need the proxy hash but don't want to commit
   * the data until after you have written an associated data item.
   * Returns the proxy hash and the data that should be written;
   * the caller is responsible for passing the pair to the HgProxyHash::store()
   * method below at the appropriate time.
   */
  static std::pair<Hash, folly::IOBuf> prepareToStore(
      RelativePathPiece path,
      Hash hgRevHash);

  /**
   * Store precomputed proxy hash information.
   * Stores the data computed by prepareToStore().
   */
  static void store(
      const std::pair<Hash, folly::IOBuf>& computedPair,
      LocalStore::WriteBatch* writeBatch);

 private:
  HgProxyHash(
      Hash edenBlobHash,
      StoreResult& infoResult,
      folly::StringPiece context);

  /**
   * Serialize the (path, hgRevHash) data into a buffer that will be stored in
   * the LocalStore.
   */
  static folly::IOBuf serialize(RelativePathPiece path, Hash hgRevHash);

  /**
   * Validate data found in value_.
   *
   * The value_ member variable should already contain the serialized data,
   * (as returned by serialize()).
   *
   * Note there will be an exception being thrown if `value_` is invalid.
   */
  void validate(Hash edenBlobHash);

  /**
   * The serialized data as written in the LocalStore.
   */
  std::string value_ = kDefaultProxyHash;
};

} // namespace eden
} // namespace facebook

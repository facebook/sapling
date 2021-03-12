/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
} // namespace folly

namespace facebook {
namespace eden {

/**
 * HgProxyHash is a derived index allowing us to map EdenFS's fixed-size hashes
 * onto Mercurial's (revHash, path) pairs.
 *
 * Mercurial doesn't really have a blob hash the same way EdenFS and Git do.
 * Instead, Mercurial file revision hashes are always relative to a specific
 * path.  To use the data in EdenFS, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the
 * blob hash in EdenFS.  We store the eden_blob_hash --> (path, hgRevHash)
 * mapping in the LocalStore.  The HgProxyHash class helps store and
 * retrieve these mappings.
 */
class HgProxyHash {
 public:
  /**
   * An uninitialized hash that contains a kZeroHash and an empty path.
   */
  HgProxyHash() = default;

  /**
   * Construct a proxy hash with encoded data. Throws an exception if the string
   * does not contain a valid HgProxyHash encoding.
   *
   * edenObjectId is only used in error messages to correlate the proxy hash
   * with Eden's object ID.
   */
  HgProxyHash(const Hash& edenObjectId, std::string value)
      : value_{std::move(value)} {
    validate(edenObjectId);
  }

  /**
   * Create a ProxyHash given the specified values.
   */
  HgProxyHash(RelativePathPiece path, const Hash& hgRevHash);

  ~HgProxyHash() = default;

  HgProxyHash(const HgProxyHash& other) = default;
  HgProxyHash& operator=(const HgProxyHash& other) = default;

  HgProxyHash(HgProxyHash&& other) noexcept
      : value_{std::exchange(other.value_, std::string{})} {}

  HgProxyHash& operator=(HgProxyHash&& other) noexcept {
    value_ = std::exchange(other.value_, std::string{});
    return *this;
  }

  RelativePathPiece path() const noexcept;

  Hash revHash() const noexcept;

  /**
   * Returns the SHA-1 of the canonical serialization of this ProxyHash, which
   * is used as the object ID throughout EdenFS.
   */
  Hash sha1() const noexcept;

  bool operator==(const HgProxyHash&) const;
  bool operator<(const HgProxyHash&) const;

  const std::string& getValue() const {
    return value_;
  }

  static folly::Future<std::vector<HgProxyHash>> getBatch(
      LocalStore* store,
      const std::vector<Hash>& blobHashes);

  /**
   * Load HgProxyHash data for the given eden blob hash from the LocalStore.
   */
  static HgProxyHash
  load(LocalStore* store, const Hash& edenObjectId, folly::StringPiece context);

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
  static std::pair<Hash, std::string> prepareToStore(
      RelativePathPiece path,
      Hash hgRevHash);

  /**
   * Store precomputed proxy hash information.
   * Stores the data computed by prepareToStore().
   */
  static void store(
      const std::pair<Hash, std::string>& computedPair,
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
  static std::string serialize(RelativePathPiece path, const Hash& hgRevHash);

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
  std::string value_;
};

} // namespace eden
} // namespace facebook

namespace std {
template <>
struct hash<facebook::eden::HgProxyHash> {
  size_t operator()(const facebook::eden::HgProxyHash& hash) const noexcept {
    return std::hash<std::string>{}(hash.getValue());
  }
};
} // namespace std

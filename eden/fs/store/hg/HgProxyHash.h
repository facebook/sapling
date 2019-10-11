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
class IOBuf;
} // namespace folly

namespace facebook {
namespace eden {

/**
 * HgProxyHash manages mercurial (path, revHash) data in the LocalStore.
 *
 * Mercurial doesn't really have a blob hash the same way eden and git do.
 * Instead, mercurial file revision hashes are always relative to a specific
 * path.  To use the data in eden, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the blob
 * hash in eden.  We store the eden_blob_hash --> (path, hgRevHash) mapping
 * in the LocalStore.  The HgProxyHash class helps store and retrieve these
 * mappings.
 */
class HgProxyHash {
 public:
  /**
   * Load HgProxyHash data for the given eden blob hash from the LocalStore.
   */
  HgProxyHash(LocalStore* store, Hash edenBlobHash, folly::StringPiece context);

  ~HgProxyHash() {}

  const RelativePathPiece& path() const {
    return path_;
  }

  const Hash& revHash() const {
    return revHash_;
  }

  static folly::Future<std::vector<std::pair<RelativePath, Hash>>> getBatch(
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
  // Not movable or copyable.
  // path_ points into value_, and would need to be updated after
  // copying/moving the data.  Since no-one needs to copy or move HgProxyHash
  // objects, we don't implement this for now.
  HgProxyHash(const HgProxyHash&) = delete;
  HgProxyHash& operator=(const HgProxyHash&) = delete;
  HgProxyHash(HgProxyHash&&) = delete;
  HgProxyHash& operator=(HgProxyHash&&) = delete;

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
   * Parse the serialized data found in value_, and set revHash_ and path_.
   *
   * The value_ member variable should already contain the serialized data,
   * (as returned by serialize()).
   *
   * Note that path_ will be set to a RelativePathPiece pointing into the
   * string data owned by value_.  (This lets us avoid copying the string data
   * out.)
   */
  void parseValue(Hash edenBlobHash);

  /**
   * The serialized data as written in the LocalStore.
   */
  std::string value_;
  /**
   * The revision hash.
   */
  Hash revHash_;
  /**
   * The path name.  Note that this points into the serialized value_ data.
   * path_ itself does not own the data it points to.
   */
  RelativePathPiece path_;
};

} // namespace eden
} // namespace facebook

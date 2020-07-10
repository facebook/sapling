/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <string>
#include <vector>

#include <folly/FixedString.h>

#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
class IOBuf;
} // namespace folly

namespace facebook {
namespace eden {

class Hash;
class LocalStore;

/**
 * ScsProxyHash manages Source Control Service data in the LocalStore.
 *
 * Source Control Service (SCS) uses different IDs to identify trees than
 * other services. Trees are identified by their commit and path, so
 * we need to keep this info around.
 *
 * We store the eden_blob_hash --> (commit hash, path) mapping in the
 * LocalStore. The ScsProxyHash class helps store and retrieve these mappings.
 */
class ScsProxyHash {
 public:
  /**
   * Load ScsProxyHash data for the given eden blob hash from the LocalStore.
   */
  static std::optional<ScsProxyHash>
  load(LocalStore* store, Hash edenBlobHash, folly::StringPiece context);

  ~ScsProxyHash() = default;

  ScsProxyHash(const ScsProxyHash& other) = default;
  ScsProxyHash& operator=(const ScsProxyHash& other) = default;

  ScsProxyHash(ScsProxyHash&& other) noexcept(false) {
    value_.swap(other.value_);
  }

  ScsProxyHash& operator=(ScsProxyHash&& other) noexcept(false) {
    value_.swap(other.value_);
    other.value_ = kDefaultProxyHash;
    return *this;
  }

  Hash commitHash() const;
  RelativePathPiece path() const;

  /**
   * Store ScsProxyHash data in the LocalStore.
   */
  static void store(
      Hash edenBlobHash,
      RelativePathPiece path,
      Hash commitHash,
      LocalStore::WriteBatch* writeBatch);

 private:
  explicit ScsProxyHash(std::string value);

  static folly::IOBuf prepareToStore(RelativePathPiece path, Hash commitHash);
  /**
   * Serialize the scsHash data into a buffer that will be stored in
   * the LocalStore.
   */
  static folly::IOBuf serialize(RelativePathPiece path, Hash commitHash);

  /**
   * The serialized data as written in the LocalStore.
   */
  std::string value_ = kDefaultProxyHash;
};
} // namespace eden
} // namespace facebook

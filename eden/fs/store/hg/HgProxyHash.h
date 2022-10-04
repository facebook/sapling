/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
} // namespace folly

namespace facebook::eden {

class EdenStats;

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
 *
 * NOTE: This class is deprecated. When support for reading the hgproxyhash
 * table in LocalStore is removed, it should be replaced with a simple
 * (hgRevHash, path) pair.
 */
class HgProxyHash {
 public:
  static std::optional<HgProxyHash> tryParseEmbeddedProxyHash(
      const ObjectId& edenObjectId);

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
  HgProxyHash(const ObjectId& edenObjectId, std::string value)
      : value_{std::move(value)} {
    validate(edenObjectId);
  }

  /**
   * Create a ProxyHash given the specified values.
   */
  HgProxyHash(RelativePathPiece path, const Hash20& hgRevHash);

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

  /**
   * Extract the hash part of the HgProxyHash and return a slice of it.
   *
   * The returned slice will live as long as this HgProxyHash.
   */
  folly::ByteRange byteHash() const noexcept;

  /**
   * Extract the hash part of the HgProxyHash and return a copy of it.
   */
  Hash20 revHash() const noexcept;

  /**
   * Returns the SHA-1 of the canonical serialization of this ProxyHash, which
   * is used as the object ID throughout EdenFS.
   */
  ObjectId sha1() const noexcept;

  bool operator==(const HgProxyHash&) const;
  bool operator<(const HgProxyHash&) const;

  const std::string& getValue() const {
    return value_;
  }

  /**
   * Load all the proxy hashes given.
   *
   * The caller is responsible for keeping the ObjectIdRange alive for the
   * duration of the future.
   */
  static folly::Future<std::vector<HgProxyHash>>
  getBatch(LocalStore* store, ObjectIdRange blobHashes, EdenStats& stats);

  /**
   * Load HgProxyHash data for the given eden blob hash from the LocalStore.
   */
  static HgProxyHash load(
      LocalStore* store,
      const ObjectId& edenObjectId,
      folly::StringPiece context,
      EdenStats& stats);

  /**
   * Encode an ObjectId from path, manifest ID, and format.
   */
  static ObjectId store(
      RelativePathPiece path,
      const Hash20& hgRevHash,
      HgObjectIdFormat hgObjectIdFormat);

  /**
   * Generate an ObjectId that contains both the hgRevHash and a path.
   */
  static ObjectId makeEmbeddedProxyHash1(
      const Hash20& hgRevHash,
      RelativePathPiece path);

  /**
   * Generate an ObjectId that contains hgRevHash directly without a path.
   */
  static ObjectId makeEmbeddedProxyHash2(const Hash20& hgRevHash);

 private:
  HgProxyHash(
      ObjectId edenBlobHash,
      StoreResult& infoResult,
      folly::StringPiece context);

  /**
   * Serialize the (path, hgRevHash) data into a buffer that will be stored in
   * the LocalStore.
   */
  static std::string serialize(RelativePathPiece path, const Hash20& hgRevHash);

  /**
   * Validate data found in value_.
   *
   * The value_ member variable should already contain the serialized data,
   * (as returned by serialize()).
   *
   * Note there will be an exception being thrown if `value_` is invalid.
   */
  void validate(ObjectId edenBlobHash);

  enum Type : uint8_t {
    // If the Object ID's type is 1, then it contains a 20-byte manifest ID
    // followed by the path. This is a temporary scheme until HgImporter is
    // gone.
    TYPE_HG_ID_WITH_PATH = 0x01,

    // If the Object ID's type is 2, its length is 21, and the remaining bytes
    // are the manifest ID. This scheme requires use of EdenSCM/EdenAPI fetches
    // that do not take a path parameter.
    TYPE_HG_ID_NO_PATH = 0x02,
  };

  /**
   * The serialized data as written in the LocalStore.
   */
  std::string value_;
};

} // namespace facebook::eden

namespace std {
template <>
struct hash<facebook::eden::HgProxyHash> {
  size_t operator()(const facebook::eden::HgProxyHash& hash) const noexcept {
    return std::hash<std::string>{}(hash.getValue());
  }
};
} // namespace std

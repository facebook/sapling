/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"

namespace facebook::eden {

class EdenStats;

/**
 * SaplingObjectId is a derived index allowing us to map EdenFS's fixed-size
 * hashes onto Mercurial's (revHash, path) pairs.
 *
 * Mercurial doesn't really have a blob hash the same way EdenFS and Git do.
 * Instead, Mercurial file revision hashes are always relative to a specific
 * path.  To use the data in EdenFS, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the
 * blob hash in EdenFS.  We store the eden_blob_hash --> (path, hgRevHash)
 * mapping in the LocalStore.  The SaplingObjectId class helps store and
 * retrieve these mappings.
 *
 * NOTE: This class is deprecated. When support for reading the hgproxyhash
 * table in LocalStore is removed, it should be replaced with a simple
 * (hgRevHash, path) pair.
 */
class SaplingObjectId {
 public:
  /**
   * An uninitialized hash that contains a kZeroHash and an empty path.
   */
  SaplingObjectId() = default;

  /**
   * Construct a proxy hash from ObjectId. Throws an exception if the oid
   * does not contain a valid embedded SaplingObjectId;
   */
  explicit SaplingObjectId(const ObjectId& edenObjectId);

  /**
   * Construct a proxy hash with encoded data. Throws an exception if the string
   * does not contain a valid SaplingObjectId encoding.
   *
   * edenObjectId is only used in error messages to correlate the proxy hash
   * with Eden's object ID.
   */
  SaplingObjectId(const ObjectId& edenObjectId, std::string value)
      : value_{std::move(value)} {
    validate(edenObjectId);
  }

  /**
   * Create a ProxyHash given the specified values.
   */
  SaplingObjectId(RelativePathPiece path, const Hash20& hgRevHash);

  ~SaplingObjectId() = default;

  SaplingObjectId(const SaplingObjectId& other) = default;
  SaplingObjectId& operator=(const SaplingObjectId& other) = default;

  SaplingObjectId(SaplingObjectId&& other) noexcept
      : value_{std::exchange(other.value_, std::string{})} {}

  SaplingObjectId& operator=(SaplingObjectId&& other) noexcept {
    value_ = std::exchange(other.value_, std::string{});
    return *this;
  }

  RelativePathPiece path() const noexcept;

  /**
   * Extract the hash part of the SaplingObjectId and return a slice of it.
   *
   * The returned slice will live as long as this SaplingObjectId.
   */
  folly::ByteRange byteHash() const noexcept;

  /**
   * Extract the hash part of the SaplingObjectId and return a copy of it.
   */
  Hash20 revHash() const noexcept;

  bool operator==(const SaplingObjectId&) const;
  bool operator<(const SaplingObjectId&) const;

  const std::string& getValue() const {
    return value_;
  }

  /**
   * Load all the proxy hashes given.
   *
   * The caller is responsible for keeping the ObjectIdRange alive for the
   * duration of the future.
   */
  static ImmediateFuture<std::vector<SaplingObjectId>> getBatch(
      ObjectIdRange blobHashes,
      bool prefetchOptimizations);

  /**
   * Encode an ObjectId from path, manifest ID, and format.
   */
  static ObjectId store(
      RelativePathPiece path,
      const Hash20& hgRevHash,
      HgObjectIdFormat hgObjectIdFormat);

  /**
   * Encode an ObjectId from path pieces, manifest ID, and format.
   * This overload avoids allocating a full path string by taking the path
   * components separately.
   */
  static ObjectId store(
      RelativePathPiece basePath,
      PathComponentPiece leafName,
      const Hash20& hgRevHash,
      HgObjectIdFormat hgObjectIdFormat);

  /**
   * Generate an ObjectId that contains both the hgRevHash and a path.
   */
  static ObjectId makeEmbeddedProxyHash1(
      const Hash20& hgRevHash,
      RelativePathPiece path);

  /**
   * Generate an ObjectId that contains both the hgRevHash and a path built from
   * path components. This overload avoids allocating a full path string.
   */
  static ObjectId makeEmbeddedProxyHash1(
      const Hash20& hgRevHash,
      RelativePathPiece basePath,
      PathComponentPiece leafName);

  /**
   * Generate an ObjectId that contains hgRevHash directly without a path.
   */
  static ObjectId makeEmbeddedProxyHash2(const Hash20& hgRevHash);

  /**
   * Return whether oid starts with a valid SaplingObjectId type byte.
   */
  static bool hasValidType(const ObjectId& oid);

 private:
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

/**
 * Shorter alias for convenience.
 */
using SlOid = SaplingObjectId;

} // namespace facebook::eden

namespace std {
template <>
struct hash<facebook::eden::SaplingObjectId> {
  size_t operator()(
      const facebook::eden::SaplingObjectId& hash) const noexcept {
    return std::hash<std::string>{}(hash.getValue());
  }
};
} // namespace std

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/FixedString.h>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"
#include "remote_execution/lib/if/gen-cpp2/common_types.h"

namespace facebook {
namespace eden {

/**
 * ReCasDigestProxyHash manages Remote Execution CAS Digest in the
 * LocalStore.
 *
 * CAS uses Digest to identify trees. Trees are identified by root
 * Digest, and Digest is defined by Remote Execution GRPC, and currnetly is
 * its hash+size.
 *
 * We store the eden_blob_hash --> Digest mapping in the
 * LocalStore. The ReCasDigestProxyHash class helps store and retrieve these
 * mappings.
 */

class ReCasDigestProxyHash {
 public:
  static constexpr uint32_t HASH_SIZE = 40;

  /**
   * Load ReCasDigestProxyHash data for the given eden blob hash from the
   * LocalStore.
   */
  static std::optional<ReCasDigestProxyHash>
  load(LocalStore* store, Hash edenBlobHash, folly::StringPiece context);

  ~ReCasDigestProxyHash() = default;

  ReCasDigestProxyHash(const ReCasDigestProxyHash& other) = default;
  ReCasDigestProxyHash& operator=(const ReCasDigestProxyHash& other) = default;

  ReCasDigestProxyHash(ReCasDigestProxyHash&& other) noexcept
      : value_{std::exchange(other.value_, std::string{})} {}

  ReCasDigestProxyHash& operator=(ReCasDigestProxyHash&& other) noexcept(
      false) {
    value_ = std::exchange(other.value_, std::string{});
    return *this;
  }

  facebook::remote_execution::TDigest digest() const;

  /**
   * Store ReCasDigestProxyHash data in the LocalStore.
   */
  static Hash store(
      facebook::remote_execution::TDigest digest,
      LocalStore::WriteBatch* writeBatch);

  /**
   * Serialize the Digest data into a buffer that will be stored in
   * the LocalStore.
   */
  static std::string serialize(
      const facebook::remote_execution::TDigest& digest);
  static facebook::remote_execution::TDigest deserialize(
      const std::string_view value);

 private:
  explicit ReCasDigestProxyHash(std::string value);
  explicit ReCasDigestProxyHash(
      const facebook::remote_execution::TDigest& digest);

  static std::pair<Hash, std::string> prepareToStore(
      facebook::remote_execution::TDigest digest);

  /**
   * The serialized data as written in the LocalStore.
   */
  std::string value_;
};
} // namespace eden
} // namespace facebook

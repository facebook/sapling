/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/recas/ReCasDigestProxyHash.h"

#include <folly/Format.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <cstdlib>
#include <exception>
#include <stdexcept>
#include <string_view>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/store/StoreResult.h"

using folly::ByteRange;
using folly::StringPiece;

namespace facebook {
namespace eden {

constexpr folly::StringPiece kEmptyHashString =
    "d3399b7262fb56cb9ed053d68db9291c410839c4";

ReCasDigestProxyHash::ReCasDigestProxyHash(std::string value)
    : value_(std::move(value)) {}

ReCasDigestProxyHash::ReCasDigestProxyHash(
    const facebook::remote_execution::TDigest& digest) {
  value_ = ReCasDigestProxyHash::serialize(digest);
}

std::optional<ReCasDigestProxyHash> ReCasDigestProxyHash::load(
    LocalStore* store,
    Hash edenBlobHash,
    folly::StringPiece context) {
  StoreResult result =
      store->get(KeySpace::ReCasDigestProxyHashFamily, edenBlobHash);
  if (!result.isValid()) {
    XLOG(DBG3) << "RE CAS Digest proxy hash received unknown proxy hash "
               << edenBlobHash.toString() << " in " << context;
    return std::nullopt;
  }
  return ReCasDigestProxyHash(result.extractValue());
}

Hash ReCasDigestProxyHash::store(
    facebook::remote_execution::TDigest digest,
    LocalStore::WriteBatch* writeBatch) {
  auto storePair = prepareToStore(digest);

  writeBatch->put(
      KeySpace::ReCasDigestProxyHashFamily,
      storePair.first,
      ByteRange{StringPiece{storePair.second}});
  return storePair.first;
}

std::pair<Hash, std::string> ReCasDigestProxyHash::prepareToStore(
    facebook::remote_execution::TDigest digest) {
  // Serialize the  digest into a buffer.
  auto buf = serialize(digest);

  // Compute the hash of the serialized buffer
  auto edenBlobHash = Hash::sha1(buf);
  return std::make_pair(edenBlobHash, std::move(buf));
}

std::string ReCasDigestProxyHash::serialize(
    const facebook::remote_execution::TDigest& digest) {
  if (ReCasDigestProxyHash::HASH_SIZE != digest.get_hash().size()) {
    std::string msg = fmt::format(
        "Digest hash ({}) length must be {}",
        digest.get_hash(),
        ReCasDigestProxyHash::HASH_SIZE);
    throw std::invalid_argument(msg);
  }

  // We serialize the data as <digest.hash>:<digest.size>
  //
  // The digest.size is stored as uint64_t.
  // The digest.hash is 40 characters hashing
  std::string buf =
      fmt::format("{}:{}", digest.get_hash(), digest.get_size_in_bytes());
  return buf;
}

facebook::remote_execution::TDigest ReCasDigestProxyHash::deserialize(
    const std::string_view value) {
  if (ReCasDigestProxyHash::HASH_SIZE > value.size()) {
    std::string msg = fmt::format(
        "Digest ({}) length must be larger than {}",
        value,
        ReCasDigestProxyHash::HASH_SIZE);
    throw std::invalid_argument(msg);
  }

  if (value.at(ReCasDigestProxyHash::HASH_SIZE) != ':') {
    std::string msg = fmt::format("Illegal CAS Digest format {}", value);
    throw std::invalid_argument(msg);
  }

  const uint64_t size = std::strtoull(
      value.substr(ReCasDigestProxyHash::HASH_SIZE + 1).data(), nullptr, 10);

  facebook::remote_execution::TDigest digest;
  digest.hash_ref() =
      std::string{value.substr(0, ReCasDigestProxyHash::HASH_SIZE)};
  digest.set_size_in_bytes(size);

  return digest;
}

facebook::remote_execution::TDigest ReCasDigestProxyHash::digest() const {
  if (value_.empty()) {
    facebook::remote_execution::TDigest digest;
    digest.hash_ref() = kEmptyHashString;
    digest.set_size_in_bytes(0);
    return digest;
  } else {
    return ReCasDigestProxyHash::deserialize(value_);
  }
};

} // namespace eden
} // namespace facebook

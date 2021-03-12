/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgProxyHash.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>

#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"

using folly::ByteRange;
using folly::Endian;
using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

HgProxyHash::HgProxyHash(RelativePathPiece path, const Hash& hgRevHash) {
  auto [hash, buf] = prepareToStore(path, hgRevHash);
  value_ = std::move(buf);
}

folly::Future<std::vector<HgProxyHash>> HgProxyHash::getBatch(
    LocalStore* store,
    const std::vector<Hash>& blobHashes) {
  auto hashCopies = std::make_shared<std::vector<Hash>>(blobHashes);
  std::vector<ByteRange> byteRanges;
  for (auto& hash : *hashCopies) {
    byteRanges.push_back(hash.getBytes());
  }
  return store->getBatch(KeySpace::HgProxyHashFamily, byteRanges)
      .thenValue([blobHashes = hashCopies](std::vector<StoreResult>&& data) {
        std::vector<HgProxyHash> results;

        for (size_t i = 0; i < blobHashes->size(); ++i) {
          results.emplace_back(HgProxyHash{
              blobHashes->at(i), data[i], "prefetchFiles getBatch"});
        }

        return results;
      });
}

HgProxyHash HgProxyHash::load(
    LocalStore* store,
    const Hash& edenObjectId,
    StringPiece context) {
  // Read the path name and file rev hash
  auto infoResult = store->get(KeySpace::HgProxyHashFamily, edenObjectId);
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash "
              << edenObjectId.toString() << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  return HgProxyHash{edenObjectId, infoResult.extractValue()};
}

Hash HgProxyHash::store(
    RelativePathPiece path,
    Hash hgRevHash,
    LocalStore::WriteBatch* writeBatch) {
  auto computedPair = prepareToStore(path, hgRevHash);
  HgProxyHash::store(computedPair, writeBatch);
  return computedPair.first;
}

std::pair<Hash, std::string> HgProxyHash::prepareToStore(
    RelativePathPiece path,
    Hash hgRevHash) {
  // Serialize the (path, hgRevHash) tuple into a buffer.
  auto buf = serialize(path, hgRevHash);

  // Compute the hash of the serialized buffer
  auto edenBlobHash = Hash::sha1(buf);

  return std::make_pair(edenBlobHash, std::move(buf));
}

void HgProxyHash::store(
    const std::pair<Hash, std::string>& computedPair,
    LocalStore::WriteBatch* writeBatch) {
  writeBatch->put(
      KeySpace::HgProxyHashFamily,
      computedPair.first,
      ByteRange{StringPiece{computedPair.second}});
}

HgProxyHash::HgProxyHash(
    Hash edenBlobHash,
    StoreResult& infoResult,
    StringPiece context) {
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash "
              << edenBlobHash.toString() << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  value_ = infoResult.extractValue();
  validate(edenBlobHash);
}

std::string HgProxyHash::serialize(
    RelativePathPiece path,
    const Hash& hgRevHash) {
  // We serialize the data as <hash_bytes><path_length><path>
  //
  // The path_length is stored as a big-endian uint32_t.
  size_t pathLength = path.value().size();
  XCHECK(pathLength <= std::numeric_limits<uint32_t>::max())
      << "path too large";

  std::string buf;
  buf.reserve(sizeof(hgRevHash) + 4 + pathLength);
  auto hashBytes = hgRevHash.getBytes();
  buf.append(reinterpret_cast<const char*>(hashBytes.data()), hashBytes.size());
  const uint32_t size = folly::Endian::big(static_cast<uint32_t>(pathLength));
  buf.append(reinterpret_cast<const char*>(&size), sizeof(size));
  buf.append(path.value().begin(), path.value().end());
  return buf;
}

RelativePathPiece HgProxyHash::path() const noexcept {
  if (value_.empty()) {
    return RelativePathPiece{};
  } else {
    XDCHECK_GE(value_.size(), Hash::RAW_SIZE + sizeof(uint32_t));
    StringPiece data{value_.data(), value_.size()};
    data.advance(Hash::RAW_SIZE + sizeof(uint32_t));
    return RelativePathPiece{data};
  }
}

Hash HgProxyHash::revHash() const noexcept {
  if (value_.empty()) {
    return kZeroHash;
  } else {
    XDCHECK_GE(value_.size(), Hash::RAW_SIZE);
    return Hash{ByteRange{StringPiece{value_.data(), Hash::RAW_SIZE}}};
  }
}

Hash HgProxyHash::sha1() const noexcept {
  if (value_.empty()) {
    // The SHA-1 of an empty HgProxyHash, (kZeroHash, "").
    // The correctness of this value is asserted in tests.
    constexpr Hash emptyProxyHash{
        folly::StringPiece{"d3399b7262fb56cb9ed053d68db9291c410839c4"}};
    return emptyProxyHash;
  } else {
    return Hash::sha1(value_);
  }
}

bool HgProxyHash::operator==(const HgProxyHash& otherHash) const {
  return value_ == otherHash.value_;
}

bool HgProxyHash::operator<(const HgProxyHash& otherHash) const {
  return value_ < otherHash.value_;
}

void HgProxyHash::validate(Hash edenBlobHash) {
  ByteRange infoBytes = StringPiece(value_);
  // Make sure the data is long enough to contain the rev hash and path length
  if (infoBytes.size() < Hash::RAW_SIZE + sizeof(uint32_t)) {
    auto msg = folly::to<string>(
        "mercurial blob info data for ",
        edenBlobHash.toString(),
        " is too short (",
        infoBytes.size(),
        " bytes)");
    XLOG(ERR) << msg;
    throw std::length_error(msg);
  }

  infoBytes.advance(Hash::RAW_SIZE);

  // Extract the path length
  uint32_t pathLength;
  memcpy(&pathLength, infoBytes.data(), sizeof(uint32_t));
  pathLength = Endian::big(pathLength);
  infoBytes.advance(sizeof(uint32_t));
  // Make sure the path length agrees with the length of data remaining
  if (infoBytes.size() != pathLength) {
    auto msg = folly::to<string>(
        "mercurial blob info data for ",
        edenBlobHash.toString(),
        " has inconsistent path length");
    XLOG(ERR) << msg;
    throw std::length_error(msg);
  }
}
} // namespace eden
} // namespace facebook

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgProxyHash.h"

#include <fmt/core.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>

#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Throw.h"

using folly::ByteRange;
using folly::Endian;
using folly::StringPiece;
using std::string;

namespace facebook::eden {

HgProxyHash::HgProxyHash(RelativePathPiece path, const Hash20& hgRevHash) {
  auto [hash, buf] = prepareToStoreLegacy(path, hgRevHash);
  value_ = std::move(buf);
}

std::optional<HgProxyHash> HgProxyHash::tryParseEmbeddedProxyHash(
    const ObjectId& edenObjectId) {
  if (edenObjectId.size() == 20) {
    // Legacy proxy hash encoding. Fall back to fetching from LocalStore.
    return std::nullopt;
  }

  if (edenObjectId.size() < 20) {
    throwf<std::invalid_argument>(
        "unsupported proxy hash format: {}",
        folly::hexlify(edenObjectId.getBytes()));
  }

  auto bytes = edenObjectId.getBytes();
  auto type = bytes[0];
  switch (type) {
    case TYPE_HG_ID_WITH_PATH:
      if (bytes.size() < 21) {
        throwf<std::invalid_argument>(
            "Invalid proxy hash size for TYPE_HG_ID_WITH_PATH: size {}",
            edenObjectId.size());
      }
      return HgProxyHash{
          RelativePathPiece{folly::StringPiece{bytes.subpiece(21)}},
          Hash20{bytes.subpiece(1, 20)}};

    case TYPE_HG_ID_NO_PATH:
      if (bytes.size() != 21) {
        throwf<std::invalid_argument>(
            "Invalid proxy hash size for TYPE_HG_ID_NO_PATH: size {}",
            edenObjectId.size());
      }
      return HgProxyHash{RelativePathPiece{}, Hash20{bytes.subpiece(1)}};
  }
  throwf<std::invalid_argument>(
      "Unknown proxy hash type: size {}, type {}", edenObjectId.size(), type);
}

folly::Future<std::vector<HgProxyHash>> HgProxyHash::getBatch(
    LocalStore* store,
    ObjectIdRange blobHashes) {
  std::vector<HgProxyHash> embedded_results;
  std::vector<ByteRange> byteRanges;
  for (const auto& hash : blobHashes) {
    if (auto embedded = tryParseEmbeddedProxyHash(hash)) {
      embedded_results.push_back(*embedded);
    } else {
      byteRanges.push_back(hash.getBytes());
    }
  }
  if (byteRanges.empty()) {
    return folly::Future<std::vector<HgProxyHash>>{std::move(embedded_results)};
  }
  return store->getBatch(KeySpace::HgProxyHashFamily, byteRanges)
      .thenValue([embedded_results,
                  byteRanges](std::vector<StoreResult>&& data) {
        std::vector<HgProxyHash> results{embedded_results};

        for (size_t i = 0; i < byteRanges.size(); ++i) {
          results.emplace_back(HgProxyHash{
              ObjectId{byteRanges.at(i)}, data[i], "prefetchFiles getBatch"});
        }

        return results;
      });
}

HgProxyHash HgProxyHash::load(
    LocalStore* store,
    const ObjectId& edenObjectId,
    StringPiece context) {
  if (auto embedded = tryParseEmbeddedProxyHash(edenObjectId)) {
    return *embedded;
  }
  // Read the path name and file rev hash
  auto infoResult = store->get(KeySpace::HgProxyHashFamily, edenObjectId);
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash " << edenObjectId
              << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  return HgProxyHash{edenObjectId, infoResult.extractValue()};
}

ObjectId HgProxyHash::store(
    RelativePathPiece path,
    const Hash20& hgRevHash,
    HgObjectIdFormat hgObjectIdFormat,
    LocalStore::WriteBatch* FOLLY_NULLABLE writeBatch) {
  switch (hgObjectIdFormat) {
    case HgObjectIdFormat::ProxyHash: {
      XCHECK(writeBatch) << "ProxyHash requires LocalStore writes";
      auto computedPair = prepareToStoreLegacy(path, hgRevHash);
      HgProxyHash::storeLegacy(computedPair, writeBatch);
      return computedPair.first;
    }
    case HgObjectIdFormat::WithPath:
      XCHECK(!writeBatch) << "non-ProxyHash does not need a WriteBatch";
      return makeEmbeddedProxyHash1(hgRevHash, path);
    case HgObjectIdFormat::HashOnly:
      XCHECK(!writeBatch) << "non-ProxyHash does not need a WriteBatch";
      return makeEmbeddedProxyHash2(hgRevHash);
  }
  EDEN_BUG() << "Unsupported hgObjectIdFormat: " << hgObjectIdFormat;
}

ObjectId HgProxyHash::makeEmbeddedProxyHash1(
    const Hash20& hgRevHash,
    RelativePathPiece path) {
  folly::StringPiece hashPiece{hgRevHash.getBytes()};
  folly::StringPiece pathPiece{path};

  folly::fbstring str;
  str.reserve(21 + pathPiece.size());
  str.push_back(TYPE_HG_ID_WITH_PATH);
  str.append(hashPiece.data(), hashPiece.size());
  str.append(pathPiece.data(), pathPiece.size());
  return ObjectId{std::move(str)};
}

ObjectId HgProxyHash::makeEmbeddedProxyHash2(const Hash20& hgRevHash) {
  folly::fbstring str;
  str.reserve(21);
  str.push_back(TYPE_HG_ID_NO_PATH);
  auto bytes = folly::StringPiece{hgRevHash.getBytes()};
  str.append(bytes.data(), bytes.size());
  return ObjectId{std::move(str)};
}

std::pair<ObjectId, std::string> HgProxyHash::prepareToStoreLegacy(
    RelativePathPiece path,
    Hash20 hgRevHash) {
  // Serialize the (path, hgRevHash) tuple into a buffer.
  auto buf = serialize(path, hgRevHash);

  // Compute the hash of the serialized buffer
  auto edenBlobHash = ObjectId::sha1(buf);

  return std::make_pair(edenBlobHash, std::move(buf));
}

void HgProxyHash::storeLegacy(
    const std::pair<ObjectId, std::string>& computedPair,
    LocalStore::WriteBatch* writeBatch) {
  writeBatch->put(
      KeySpace::HgProxyHashFamily,
      computedPair.first,
      ByteRange{StringPiece{computedPair.second}});
}

HgProxyHash::HgProxyHash(
    ObjectId edenBlobHash,
    StoreResult& infoResult,
    StringPiece context) {
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash " << edenBlobHash
              << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  value_ = infoResult.extractValue();
  validate(edenBlobHash);
}

std::string HgProxyHash::serialize(
    RelativePathPiece path,
    const Hash20& hgRevHash) {
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
    XDCHECK_GE(value_.size(), Hash20::RAW_SIZE + sizeof(uint32_t));
    StringPiece data{value_.data(), value_.size()};
    data.advance(Hash20::RAW_SIZE + sizeof(uint32_t));
    // value_ was built with a known good RelativePath, thus we don't need to
    // recheck it when deserializing.
    return RelativePathPiece{data, detail::SkipPathSanityCheck{}};
  }
}

ByteRange HgProxyHash::byteHash() const noexcept {
  if (value_.empty()) {
    return kZeroHash.getBytes();
  } else {
    XDCHECK_GE(value_.size(), Hash20::RAW_SIZE);
    return ByteRange{StringPiece{value_.data(), Hash20::RAW_SIZE}};
  }
}

Hash20 HgProxyHash::revHash() const noexcept {
  return Hash20{byteHash()};
}

ObjectId HgProxyHash::sha1() const noexcept {
  if (value_.empty()) {
    // The SHA-1 of an empty HgProxyHash, (kZeroHash, "").
    // The correctness of this value is asserted in tests.
    const ObjectId emptyProxyHash = ObjectId::fromHex(
        folly::StringPiece{"d3399b7262fb56cb9ed053d68db9291c410839c4"});
    return emptyProxyHash;
  } else {
    return ObjectId::sha1(value_);
  }
}

bool HgProxyHash::operator==(const HgProxyHash& otherHash) const {
  return value_ == otherHash.value_;
}

bool HgProxyHash::operator<(const HgProxyHash& otherHash) const {
  return value_ < otherHash.value_;
}

void HgProxyHash::validate(ObjectId edenBlobHash) {
  ByteRange infoBytes = StringPiece(value_);
  // Make sure the data is long enough to contain the rev hash and path length
  if (infoBytes.size() < Hash20::RAW_SIZE + sizeof(uint32_t)) {
    auto msg = folly::to<string>(
        "mercurial blob info data for ",
        edenBlobHash,
        " is too short (",
        infoBytes.size(),
        " bytes)");
    XLOG(ERR) << msg;
    throw std::length_error(msg);
  }

  infoBytes.advance(Hash20::RAW_SIZE);

  // Extract the path length
  uint32_t pathLength;
  memcpy(&pathLength, infoBytes.data(), sizeof(uint32_t));
  pathLength = Endian::big(pathLength);
  infoBytes.advance(sizeof(uint32_t));
  // Make sure the path length agrees with the length of data remaining
  if (infoBytes.size() != pathLength) {
    auto msg = folly::to<string>(
        "mercurial blob info data for ",
        edenBlobHash,
        " has inconsistent path length");
    XLOG(ERR) << msg;
    throw std::length_error(msg);
  }
}

} // namespace facebook::eden

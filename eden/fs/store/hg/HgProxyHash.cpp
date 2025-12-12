/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgProxyHash.h"

#include <fmt/core.h>
#include <folly/logging/xlog.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/Throw.h"

using folly::ByteRange;
using folly::Endian;
using folly::StringPiece;
using std::string;

namespace facebook::eden {

HgProxyHash::HgProxyHash(RelativePathPiece path, const Hash20& hgRevHash)
    : value_{serialize(path, hgRevHash)} {}

HgProxyHash::HgProxyHash(const ObjectId& edenObjectId) {
  if (edenObjectId.size() <= 20) {
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
      value_ = serialize(
          RelativePathPiece{folly::StringPiece{bytes.subpiece(21)}},
          Hash20{bytes.subpiece(1, 20)});
      break;
    case TYPE_HG_ID_NO_PATH:
      if (bytes.size() != 21) {
        throwf<std::invalid_argument>(
            "Invalid proxy hash size for TYPE_HG_ID_NO_PATH: size {}",
            edenObjectId.size());
      }
      value_ = serialize(RelativePathPiece{}, Hash20{bytes.subpiece(1)});
      break;
    default:
      throwf<std::invalid_argument>(
          "Unknown proxy hash type: size {}, type {}",
          edenObjectId.size(),
          type);
  }
}

ImmediateFuture<std::vector<HgProxyHash>> HgProxyHash::getBatch(
    ObjectIdRange blobHashes,
    bool prefetchOptimizations) {
  auto processBatch = [blobHashes]() {
    std::vector<HgProxyHash> results;
    results.reserve(blobHashes.size());
    for (size_t index = 0; index < blobHashes.size(); index++) {
      results.emplace_back(blobHashes.at(index));
    }

    return ImmediateFuture<std::vector<HgProxyHash>>{std::move(results)};
  };

  constexpr size_t kAsyncThreshold = 1000;

  // If over the threshold, force the ObjectId->HgProxyHash conversion to be
  // async.
  if (prefetchOptimizations && blobHashes.size() > kAsyncThreshold) {
    return makeNotReadyImmediateFuture().thenValue(
        [processBatch = std::move(processBatch)](auto&&) {
          return processBatch();
        });
  } else {
    return processBatch();
  }
}

ObjectId HgProxyHash::store(
    RelativePathPiece path,
    const Hash20& hgRevHash,
    HgObjectIdFormat hgObjectIdFormat) {
  switch (hgObjectIdFormat) {
    case HgObjectIdFormat::WithPath:
      return makeEmbeddedProxyHash1(hgRevHash, path);
    case HgObjectIdFormat::HashOnly:
      return makeEmbeddedProxyHash2(hgRevHash);
  }
  EDEN_BUG() << "Unsupported hgObjectIdFormat: "
             << fmt::underlying(hgObjectIdFormat);
}

ObjectId HgProxyHash::store(
    RelativePathPiece basePath,
    PathComponentPiece leafName,
    const Hash20& hgRevHash,
    HgObjectIdFormat hgObjectIdFormat) {
  switch (hgObjectIdFormat) {
    case HgObjectIdFormat::WithPath:
      return makeEmbeddedProxyHash1(hgRevHash, basePath, leafName);
    case HgObjectIdFormat::HashOnly:
      return makeEmbeddedProxyHash2(hgRevHash);
  }
  EDEN_BUG() << "Unsupported hgObjectIdFormat: "
             << fmt::underlying(hgObjectIdFormat);
}

ObjectId HgProxyHash::makeEmbeddedProxyHash1(
    const Hash20& hgRevHash,
    RelativePathPiece path) {
  folly::StringPiece hashPiece{hgRevHash.getBytes()};
  std::string_view pathPiece{path};

  folly::fbstring str;
  str.reserve(21 + pathPiece.size());
  str.push_back(TYPE_HG_ID_WITH_PATH);
  str.append(hashPiece.data(), hashPiece.size());
  str.append(pathPiece.data(), pathPiece.size());
  return ObjectId{std::move(str)};
}

ObjectId HgProxyHash::makeEmbeddedProxyHash1(
    const Hash20& hgRevHash,
    RelativePathPiece basePath,
    PathComponentPiece leafName) {
  folly::StringPiece hashPiece{hgRevHash.getBytes()};
  std::string_view basePathPiece{basePath};
  std::string_view leafNamePiece{leafName};

  folly::fbstring str;
  str.reserve(21 + basePathPiece.size() + 1 + leafNamePiece.size());
  str.push_back(TYPE_HG_ID_WITH_PATH);
  str.append(hashPiece.data(), hashPiece.size());
  str.append(basePathPiece.data(), basePathPiece.size());
  if (!basePathPiece.empty()) {
    str.push_back(kDirSeparator);
  }
  str.append(leafNamePiece.data(), leafNamePiece.size());
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

bool HgProxyHash::hasValidType(const ObjectId& oid) {
  folly::ByteRange bytes = oid.getBytes();
  // 20 bytes is a legacy proxy hash (with no type byte).
  // >=21 bytes is a oid with embedded hg info (and a type byte).
  return bytes.size() == 20 ||
      (bytes.size() >= 21 &&
       (bytes[0] == TYPE_HG_ID_WITH_PATH || bytes[0] == TYPE_HG_ID_NO_PATH));
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
    auto msg = fmt::format(
        "mercurial blob info data for {} is too short ({} bytes)",
        edenBlobHash,
        infoBytes.size());
    XLOG(ERR, msg);
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
    auto msg = fmt::format(
        "mercurial blob info data for {} has inconsistent path length",
        edenBlobHash);
    XLOG(ERR, msg);
    throw std::length_error(msg);
  }
}

} // namespace facebook::eden

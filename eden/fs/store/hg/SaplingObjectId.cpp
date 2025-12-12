/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/SaplingObjectId.h"

#include <fmt/core.h>
#include <folly/logging/xlog.h>

#include "eden/common/utils/Throw.h"

using folly::ByteRange;
using folly::Endian;
using folly::StringPiece;
using std::string;

namespace facebook::eden {

// SlOid prefix length (type byte and 20 byte node)
constexpr size_t slOidLenSansPath = Hash20::RAW_SIZE + 1;

SaplingObjectId::SaplingObjectId(const Hash20& slHash, RelativePathPiece path) {
  value_.reserve(slOidLenSansPath + path.view().size());
  value_.push_back(TYPE_HG_ID_WITH_PATH);
  value_.append((const char*)slHash.getBytes().data(), slHash.RAW_SIZE);
  value_.append(path.view());
}

SaplingObjectId::SaplingObjectId(
    const Hash20& slHash,
    RelativePathPiece dir,
    PathComponentPiece name) {
  value_.reserve(slOidLenSansPath + dir.view().size() + 1 + name.view().size());
  value_.push_back(TYPE_HG_ID_WITH_PATH);
  value_.append((const char*)slHash.getBytes().data(), slHash.RAW_SIZE);
  if (!dir.empty()) {
    value_.append(dir.view());
    value_.push_back(kDirSeparator);
  }
  value_.append(name.view());
}

SaplingObjectId::SaplingObjectId(const Hash20& slHash) {
  value_.reserve(slOidLenSansPath);
  value_.push_back(TYPE_HG_ID_NO_PATH);
  value_.append((const char*)slHash.getBytes().data(), slHash.RAW_SIZE);
}

SaplingObjectId::SaplingObjectId(folly::StringPiece value) : value_{value} {
  validate();
}

SaplingObjectId::SaplingObjectId(const ObjectId& oid) : value_{oid.getBytes()} {
  validate();
}

ObjectId SaplingObjectId::oid() && {
  return ObjectId{std::move(value_)};
}

ImmediateFuture<std::vector<SaplingObjectId>> SaplingObjectId::getBatch(
    ObjectIdRange blobHashes,
    bool prefetchOptimizations) {
  auto processBatch = [blobHashes]() {
    std::vector<SaplingObjectId> results;
    results.reserve(blobHashes.size());
    for (size_t index = 0; index < blobHashes.size(); index++) {
      results.emplace_back(blobHashes.at(index));
    }

    return ImmediateFuture<std::vector<SaplingObjectId>>{std::move(results)};
  };

  constexpr size_t kAsyncThreshold = 1000;

  // If over the threshold, force the ObjectId->SaplingObjectId conversion to be
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

bool SaplingObjectId::hasValidType(const ObjectId& oid) {
  folly::ByteRange bytes = oid.getBytes();
  // 20 bytes is a legacy proxy hash (with no type byte).
  // >=21 bytes is a oid with embedded hg info (and a type byte).
  return bytes.size() == 20 ||
      (bytes.size() >= 21 &&
       (bytes[0] == TYPE_HG_ID_WITH_PATH || bytes[0] == TYPE_HG_ID_NO_PATH));
}

RelativePathPiece SaplingObjectId::path() const noexcept {
  XDCHECK((validate(), true));
  if (value_.empty() || value_[0] == TYPE_HG_ID_NO_PATH) {
    return RelativePathPiece{};
  } else {
    // value_ was built with a known good RelativePath, or validated on
    // construction. We can skip the sanity check here.
    return RelativePathPiece{
        std::string_view{value_}.substr(slOidLenSansPath),
        detail::SkipPathSanityCheck{}};
  }
}

Hash20& SaplingObjectId::node() const noexcept {
  XDCHECK((validate(), true));
  if (value_.empty()) {
    return const_cast<Hash20&>(kZeroHash);
  } else {
    return *reinterpret_cast<Hash20*>(const_cast<char*>(value_.data() + 1));
  }
}

bool SaplingObjectId::operator==(const SaplingObjectId& otherHash) const {
  return value_ == otherHash.value_;
}

bool SaplingObjectId::operator<(const SaplingObjectId& otherHash) const {
  return value_ < otherHash.value_;
}

void SaplingObjectId::validate() const {
  if (value_.empty()) {
    // Special case - empty value is okay.
    return;
  }

  auto type = value_[0];
  switch (type) {
    case TYPE_HG_ID_WITH_PATH:
      if (value_.size() < slOidLenSansPath) {
        throwf<std::invalid_argument>(
            "Invalid SaplingObjectId size for TYPE_HG_ID_WITH_PATH: size {}",
            value_.size());
      }
      // Validate the path.
      (void)RelativePathPiece{
          std::string_view{value_}.substr(slOidLenSansPath)};
      break;
    case TYPE_HG_ID_NO_PATH:
      if (value_.size() != slOidLenSansPath) {
        throwf<std::invalid_argument>(
            "Invalid SaplingObjectId size for TYPE_HG_ID_NO_PATH: size {}",
            value_.size());
      }
      break;
    default:
      throwf<std::invalid_argument>(
          "Unknown SaplingObjectId type: size {}, type {}",
          value_.size(),
          type);
  }
}

} // namespace facebook::eden

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <rust/cxx.h>
#include <string>
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

class SaplingObjectIdView;

/**
 * SaplingObjectId represents SaplingBackingStore's ObjectId format, which
 * embeds the 20 byte Sapling hash and optionally the file/tree path.
 */
class SaplingObjectId {
 public:
  /**
   * An uninitialized SaplingObjectId that contains a kZeroHash and an empty
   * path.
   */
  SaplingObjectId() = default;

  /**
   * Construct a SaplingObjectId from an ObjectId. Throws an exception if
   * oid does not contain a valid SaplingObjectId.
   */
  explicit SaplingObjectId(const ObjectId& oid);

  /**
   * Construct a SaplingObjectId from a StringPiece. Throws an exception if
   * oid does not contain a valid SaplingObjectId.
   */
  explicit SaplingObjectId(folly::StringPiece value, bool validate = true);

  /**
   * Construct a SaplingObjectId from constituent hash and path. Encodes type as
   * TYPE_HG_ID_WITH_PATH.
   */
  SaplingObjectId(const Hash20& slHash, RelativePathPiece path);

  /**
   * Construct a SaplingObjectId from constituent hash and dir+name. Encodes
   * type as TYPE_HG_ID_WITH_PATH.
   */
  SaplingObjectId(
      const Hash20& slHash,
      RelativePathPiece dir,
      PathComponentPiece name);

  /**
   * Construct a SaplingObjectId from hash only. Encodes type as
   * TYPE_HG_ID_NO_PATH.
   */
  explicit SaplingObjectId(const Hash20& slHash);

  /**
   * Construct a SaplingObjectId by copying from a SaplingObjectIdView.
   */
  /* implicit */ SaplingObjectId(const SaplingObjectIdView& view);

  ~SaplingObjectId() = default;

  SaplingObjectId(const SaplingObjectId& other) = default;
  SaplingObjectId& operator=(const SaplingObjectId& other) = default;

  SaplingObjectId(SaplingObjectId&& other) noexcept
      : value_{std::exchange(other.value_, std::string{})} {}

  SaplingObjectId& operator=(SaplingObjectId&& other) noexcept {
    value_ = std::exchange(other.value_, std::string{});
    return *this;
  }

  /**
   * Turn this SaplingObjectId into an ObjectId.
   */
  ObjectId oid() &&;

  /**
   * Return a reference to the path part of the SaplingObjectId, or empty if not
   * present.
   */
  RelativePathPiece path() const noexcept;

  /**
   * Return a reference to the node (AKA hash) part of the SaplingObjectId.
   */
  Hash20& node() const noexcept;

  folly::ByteRange data() const {
    return folly::ByteRange{value_};
  }

  SaplingObjectIdView view() const;

  rust::Slice<const uint8_t> rustData() const {
    return rust::Slice<const uint8_t>{
        reinterpret_cast<const uint8_t*>(value_.data()), value_.size()};
  }

  bool operator==(const SaplingObjectId&) const;
  bool operator<(const SaplingObjectId&) const;

  const folly::fbstring& getValue() const {
    return value_;
  }

  /**
   * Return whether oid starts with a valid SaplingObjectId type byte.
   */
  static bool hasValidType(const ObjectId& oid);

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

 private:
  /**
   * The serialized data as written in ObjectId.
   */
  folly::fbstring value_;
};

/**
 * Shorter alias for convenience.
 */
using SlOid = SaplingObjectId;

/**
 * Validate data found in a SaplingObjectId value string.
 *
 * Throws exception if value is invalid.
 */
void validateSlOid(folly::StringPiece value);

/**
 * SaplingObjectIdView provides non-owning access to a SaplingObjectId stored
 * in an ObjectId, without allocating or copying.
 */
class SaplingObjectIdView {
 public:
  /**
   * Construct a view from an ObjectId reference. The ObjectId must outlive
   * this view and must contain a valid SaplingObjectId.
   */
  explicit SaplingObjectIdView(const ObjectId& oid);

  /**
   * Construct a view from a SaplingObjectId reference. The SaplingObjectId
   * must outlive this view.
   */
  /* implicit */ SaplingObjectIdView(const SaplingObjectId& slOid);

  explicit SaplingObjectIdView(folly::ByteRange data);

  RelativePathPiece path() const noexcept;

  Hash20& node() const noexcept;

  folly::ByteRange data() const {
    return value_;
  }

  rust::Slice<const uint8_t> rustData() const {
    return rust::Slice<const uint8_t>{value_.data(), value_.size()};
  }

 private:
  const folly::ByteRange value_;
};

/**
 * Shorter alias for convenience.
 */
using SlOidView = SaplingObjectIdView;

} // namespace facebook::eden

namespace std {
template <>
struct hash<facebook::eden::SaplingObjectId> {
  size_t operator()(
      const facebook::eden::SaplingObjectId& hash) const noexcept {
    return std::hash<folly::fbstring>{}(hash.getValue());
  }
};
} // namespace std

template <>
struct fmt::formatter<facebook::eden::SaplingObjectId> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename Context>
  auto format(const facebook::eden::SaplingObjectId& oid, Context& ctx) const {
    return fmt::format_to(
        ctx.out(), "{}", facebook::eden::SaplingObjectIdView{oid.data()});
  }
};

template <>
struct fmt::formatter<facebook::eden::SaplingObjectIdView> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename Context>
  auto format(const facebook::eden::SaplingObjectIdView& oid, Context& ctx)
      const {
    auto out = ctx.out();
    out = fmt::format_to(out, "node={}", oid.node());
    auto bytes = oid.data();
    if (!bytes.empty() &&
        bytes[0] == facebook::eden::SaplingObjectId::TYPE_HG_ID_WITH_PATH) {
      out = fmt::format_to(out, " path={}", oid.path());
    }
    return std::move(out);
  }
};

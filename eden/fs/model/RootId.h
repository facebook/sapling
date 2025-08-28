/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include "eden/fs/model/Hash.h"

namespace facebook::eden {

/**
 * Each BackingStore implementation defines the meaning of its root. For
 * example, for Mercurial, that's a 20-byte commit hash. Roots may have a
 * different representation than tree IDs, so allow the BackingStore to define
 * the semantics.
 *
 * RootId should generally be human-readable (e.g. hex strings) because it is
 * printed to logs with C escaping rules.
 */
class RootId {
 public:
  RootId() = default;

  explicit RootId(std::string id) : id_{std::move(id)} {}
  RootId(const RootId&) = default;
  RootId(RootId&&) = default;

  RootId& operator=(const RootId&) = default;
  RootId& operator=(RootId&&) = default;

  const std::string& value() const {
    return id_;
  }

  friend bool operator==(const RootId& lhs, const RootId& rhs) {
    return lhs.id_ == rhs.id_;
  }

  friend bool operator!=(const RootId& lhs, const RootId& rhs) {
    return lhs.id_ != rhs.id_;
  }

  friend bool operator<(const RootId& lhs, const RootId& rhs) {
    return lhs.id_ < rhs.id_;
  }

 private:
  std::string id_;
};

/**
 * The meaning of a RootId is defined by the BackingStore implementation. Allow
 * it to also define how root IDs are parsed and rendered at API boundaries such
 * as Thrift.
 */
class RootIdCodec {
 public:
  virtual ~RootIdCodec() = default;
  virtual RootId parseRootId(folly::StringPiece rootId) = 0;
  virtual std::string renderRootId(const RootId& rootId) = 0;
};

} // namespace facebook::eden

namespace std {

template <>
struct hash<facebook::eden::RootId> {
  std::size_t operator()(const facebook::eden::RootId& rootId) const {
    return std::hash<std::string>{}(rootId.value());
  }
};

} // namespace std

template <>
struct fmt::formatter<facebook::eden::RootId> : formatter<std::string> {
  template <typename Context>
  auto format(const facebook::eden::RootId& id, Context& ctx) const {
    // no extra allocation due to RootId::value returning a const std::string&
    return formatter<std::string>::format(id.value(), ctx);
  }
};

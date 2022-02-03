/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ObjectFetchContext.h"

namespace {
using namespace facebook::eden;

class NullObjectFetchContext : public ObjectFetchContext {
 public:
  NullObjectFetchContext() = default;
  explicit NullObjectFetchContext(std::optional<folly::StringPiece> causeDetail)
      : causeDetail_(causeDetail) {}

  std::optional<folly::StringPiece> getCauseDetail() const override {
    return causeDetail_;
  }

  const std::optional<std::unordered_map<std::string, std::string>>&
  getRequestInfo() const override {
    return requestInfo_;
  }

 private:
  std::optional<folly::StringPiece> causeDetail_;
  std::optional<std::unordered_map<std::string, std::string>> requestInfo_;
};
} // namespace

namespace facebook::eden {

ObjectFetchContext& ObjectFetchContext::getNullContext() {
  static auto* p = new NullObjectFetchContext;
  return *p;
}

ObjectFetchContext* ObjectFetchContext::getNullContextWithCauseDetail(
    folly::StringPiece causeDetail) {
  return new NullObjectFetchContext(folly::StringPiece{causeDetail});
}

} // namespace facebook::eden

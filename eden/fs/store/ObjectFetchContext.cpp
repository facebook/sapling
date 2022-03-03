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

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

 private:
  std::optional<folly::StringPiece> causeDetail_;
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

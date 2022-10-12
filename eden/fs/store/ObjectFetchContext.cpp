/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ObjectFetchContext.h"
#include <folly/CppAttributes.h>

namespace {

using namespace facebook::eden;

class NullObjectFetchContext : public ObjectFetchContext {
 public:
  NullObjectFetchContext() = default;

  explicit NullObjectFetchContext(std::optional<std::string_view> causeDetail)
      : causeDetail_(causeDetail) {}

  Cause getCause() const override {
    return Cause::Unknown;
  }

  std::optional<std::string_view> getCauseDetail() const override {
    return causeDetail_;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

 private:
  std::optional<std::string_view> causeDetail_;
};

} // namespace

namespace facebook::eden {

ObjectFetchContext& ObjectFetchContext::getNullContext() {
  static auto* p = new NullObjectFetchContext;
  return *p;
}

ObjectFetchContext* ObjectFetchContext::getNullContextWithCauseDetail(
    std::string_view causeDetail) {
  return new NullObjectFetchContext(causeDetail);
}

} // namespace facebook::eden

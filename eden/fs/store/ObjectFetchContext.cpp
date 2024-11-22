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

class NullFSObjectFetchContext : public ObjectFetchContext {
 public:
  NullFSObjectFetchContext() = default;

  Cause getCause() const override {
    return Cause::Fs;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }
};

class NullPrefetchObjectFetchContext : public ObjectFetchContext {
 public:
  NullPrefetchObjectFetchContext() = default;

  Cause getCause() const override {
    return Cause::Prefetch;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }
};

} // namespace

namespace facebook::eden {

ObjectFetchContextPtr ObjectFetchContext::getNullContext() {
  static auto* p = new NullObjectFetchContext;
  return ObjectFetchContextPtr::singleton(*p);
}

ObjectFetchContextPtr ObjectFetchContext::getNullContextWithCauseDetail(
    std::string_view causeDetail) {
  return ObjectFetchContextPtr::singleton(
      *new NullObjectFetchContext{causeDetail});
}

ObjectFetchContextPtr ObjectFetchContext::getNullFsContext() {
  static auto* p = new NullFSObjectFetchContext;
  return ObjectFetchContextPtr::singleton(*p);
}

ObjectFetchContextPtr ObjectFetchContext::getNullPrefetchContext() {
  static auto* p = new NullPrefetchObjectFetchContext;
  return ObjectFetchContextPtr::singleton(*p);
}

} // namespace facebook::eden

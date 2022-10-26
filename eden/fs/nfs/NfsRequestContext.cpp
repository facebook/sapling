/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/NfsRequestContext.h"
#include <folly/Utility.h>

namespace facebook::eden {

namespace {

class NfsObjectFetchContext : public FsObjectFetchContext {
 public:
  explicit NfsObjectFetchContext(std::string_view causeDetail)
      : causeDetail_{causeDetail} {}

  std::optional<std::string_view> getCauseDetail() const override {
    return causeDetail_;
  }

 private:
  std::string_view causeDetail_;
};

} // namespace

NfsRequestContext::NfsRequestContext(
    uint32_t xid,
    std::string_view causeDetail,
    ProcessAccessLog& processAccessLog)
    : RequestContext{processAccessLog, std::make_shared<NfsObjectFetchContext>(causeDetail)},
      xid_{xid} {}

} // namespace facebook::eden

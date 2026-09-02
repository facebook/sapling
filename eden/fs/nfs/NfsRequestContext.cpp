/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/NfsRequestContext.h"

namespace facebook::eden {

namespace {

class NfsObjectFetchContext : public FsObjectFetchContext {
 public:
  NfsObjectFetchContext(
      std::string_view causeDetail,
      std::optional<uint32_t> clientUid,
      std::optional<uint32_t> clientGid)
      : causeDetail_{causeDetail},
        clientUid_{clientUid},
        clientGid_{clientGid} {}

  std::optional<std::string_view> getCauseDetail() const override {
    return causeDetail_;
  }

  std::optional<uint32_t> getClientUid() const override {
    return clientUid_;
  }

  std::optional<uint32_t> getClientGid() const override {
    return clientGid_;
  }

 private:
  std::string_view causeDetail_;
  std::optional<uint32_t> clientUid_;
  std::optional<uint32_t> clientGid_;
};

using NfsObjectFetchContextPtr = RefPtr<NfsObjectFetchContext>;

} // namespace

NfsRequestContext::NfsRequestContext(
    uint32_t xid,
    std::string_view causeDetail,
    ProcessAccessLog& processAccessLog,
    std::shared_ptr<EdenFsEventsLogger> edenFsEventsLogger,
    std::chrono::nanoseconds longRunningFsRequestThreshold,
    const std::optional<authsys_parms>& authSysCreds)
    : RequestContext{processAccessLog, std::move(edenFsEventsLogger), longRunningFsRequestThreshold, makeRefPtr<NfsObjectFetchContext>(causeDetail, authSysCreds ? std::optional{authSysCreds->uid} : std::nullopt, authSysCreds ? std::optional{authSysCreds->gid} : std::nullopt)},
      xid_{xid} {}

} // namespace facebook::eden

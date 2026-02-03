/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include <optional>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/prjfs/PrjfsChannel.h"

namespace facebook::eden {

class PrjfsObjectFetchContext : public FsObjectFetchContext {
 public:
  explicit PrjfsObjectFetchContext(ProcessId pid) : pid_{pid} {}

  OptionalProcessId getClientPid() const override {
    return pid_;
  }

 private:
  ProcessId pid_;
};

class PrjfsRequestContext : public RequestContext {
 public:
  PrjfsRequestContext(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext& operator=(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext(PrjfsRequestContext&&) = delete;
  PrjfsRequestContext& operator=(PrjfsRequestContext&&) = delete;

  explicit PrjfsRequestContext(
      folly::ReadMostlySharedPtr<PrjfsChannelInner> channel,
      const PRJ_CALLBACK_DATA& prjfsData,
      PrjfsTraceCallType callType,
      LPCWSTR destinationFileName);

  virtual ~PrjfsRequestContext();

  folly::ReadMostlyWeakPtr<PrjfsChannelInner> getChannelForAsyncUse();

  ImmediateFuture<folly::Unit> catchErrors(
      ImmediateFuture<folly::Unit>&& fut,
      EdenStatsPtr stats,
      StatsGroupBase::Counter PrjfsStats::* countSuccessful,
      StatsGroupBase::Counter PrjfsStats::* countFailure);

  void sendSuccess() const;

  void sendNotificationSuccess() const;

  void sendEnumerationSuccess(PRJ_DIR_ENTRY_BUFFER_HANDLE buffer) const;

  void sendError(HRESULT result) const;

 private:
  static std::string formatTraceEventString(
      PrjfsTraceCallType callType,
      const PrjfsTraceEvent::PrjfsOperationData& data,
      const PRJ_CALLBACK_DATA& prjfsData,
      LPCWSTR destinationFileName);

  static std::string processPathToName(PCWSTR fullAppName);

  static std::wstring_view basenameFromAppName(PCWSTR fullAppName);

  folly::ReadMostlySharedPtr<PrjfsChannelInner> channel_;
  int32_t commandId_;
  PrjfsTraceCallType callType_;
  PrjfsTraceEvent::PrjfsOperationData data_;
  mutable std::optional<HRESULT> result_;
};

} // namespace facebook::eden

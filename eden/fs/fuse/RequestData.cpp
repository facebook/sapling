/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/fuse/RequestData.h"

#include <folly/logging/xlog.h>

#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;
using namespace std::chrono;

namespace facebook {
namespace eden {

const std::string RequestData::kKey("fuse");

RequestData::RequestData(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader,
    Dispatcher* dispatcher)
    : channel_(channel), fuseHeader_(fuseHeader), dispatcher_(dispatcher) {}

bool RequestData::isFuseRequest() {
  return folly::RequestContext::get()->getContextData(kKey) != nullptr;
}

RequestData& RequestData::get() {
  const auto data = folly::RequestContext::get()->getContextData(kKey);
  if (UNLIKELY(!data)) {
    XLOG(FATAL) << "boom for missing RequestData";
    throw std::runtime_error("no fuse request data set in this context!");
  }
  return *dynamic_cast<RequestData*>(data);
}

RequestData& RequestData::create(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader,
    Dispatcher* dispatcher) {
  folly::RequestContext::get()->setContextData(
      RequestData::kKey,
      std::make_unique<RequestData>(channel, fuseHeader, dispatcher));
  return get();
}

void RequestData::startRequest(
    EdenStats* stats,
    FuseThreadStats::HistogramPtr histogram) {
  startTime_ = steady_clock::now();
  DCHECK(latencyHistogram_ == nullptr);
  latencyHistogram_ = histogram;
  stats_ = stats;
}

void RequestData::finishRequest() {
  const auto now = steady_clock::now();
  const auto now_since_epoch = duration_cast<seconds>(now.time_since_epoch());

  const auto diff = now - startTime_;
  const auto diff_us = duration_cast<microseconds>(diff);
  const auto diff_ns = duration_cast<nanoseconds>(diff);

  stats_->getFuseStatsForCurrentThread().recordLatency(
      latencyHistogram_, diff_us, now_since_epoch);
  latencyHistogram_ = nullptr;
  stats_ = nullptr;

  auto& pal = channel_->getProcessAccessLog();
  if (getEdenTopStats().didImportFromBackingStore()) {
    auto type = ProcessAccessLog::AccessType::FuseBackingStoreImport;
    pal.recordAccess(examineReq().pid, type);
  }
  pal.recordDuration(examineReq().pid, diff_ns);
}

fuse_in_header RequestData::stealReq() {
  if (fuseHeader_.opcode == 0) {
    throw std::runtime_error("req_ has been released");
  }
  fuse_in_header res = fuseHeader_;
  fuseHeader_.opcode = 0;
  return res;
}

const fuse_in_header& RequestData::getReq() const {
  if (fuseHeader_.opcode == 0) {
    throw std::runtime_error("req_ has been released");
  }
  return fuseHeader_;
}

const fuse_in_header& RequestData::examineReq() const {
  // Will just return the fuseHeader_ and not throw(unlike getReq)
  // The caller is responsible to check the opcode and ignore if zero
  return fuseHeader_;
}

Dispatcher* RequestData::getDispatcher() const {
  return dispatcher_;
}

RequestData::EdenTopStats& RequestData::getEdenTopStats() {
  return edenTopStats_;
}

void RequestData::replyError(int err) {
  channel_->replyError(stealReq(), err);
}

void RequestData::replyNone() {
  stealReq();
}

void RequestData::systemErrorHandler(const std::system_error& err) {
  int errnum = EIO;
  if (isErrnoError(err)) {
    errnum = err.code().value();
  }
  XLOG(DBG5) << folly::exceptionStr(err);
  RequestData::get().replyError(errnum);
}

void RequestData::genericErrorHandler(const std::exception& err) {
  XLOG(DBG5) << folly::exceptionStr(err);
  RequestData::get().replyError(EIO);
}

void RequestData::timeoutErrorHandler(const folly::FutureTimeout& err) {
  XLOG(DBG5) << folly::exceptionStr(err);
  RequestData::get().replyError(ETIMEDOUT);
}

} // namespace eden
} // namespace facebook

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/FuseRequestContext.h"

#include <folly/logging/xlog.h>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/utils/SystemError.h"
#include "eden/fs/notifications/Notifier.h"

using namespace folly;

namespace facebook::eden {

FuseRequestContext::FuseRequestContext(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader)
    : RequestContext(
          channel->getProcessAccessLog(),
          channel->getStructuredLogger(),
          channel->getLongRunningFSRequestThreshold(),
          makeRefPtr<FuseObjectFetchContext>(
              ProcessId{fuseHeader.pid},
              fuseHeader.opcode)),
      channel_(channel),
      fuseHeader_(fuseHeader) {}

fuse_in_header FuseRequestContext::stealReqWithResult(int64_t result) {
  if (result_.has_value()) {
    throw std::runtime_error("req_ has been released");
  }
  result_ = result;
  return fuseHeader_;
}

const fuse_in_header& FuseRequestContext::getReq() const {
  if (result_.has_value()) {
    throw std::runtime_error("req_ has been released");
  }
  return fuseHeader_;
}

void FuseRequestContext::systemErrorHandler(
    const std::system_error& err,
    Notifier* FOLLY_NULLABLE notifier) {
  int errnum = EIO;
  if (isErrnoError(err)) {
    errnum = err.code().value();
  }
  XLOG(DBG5, folly::exceptionStr(err));
  replyError(errnum);
  if (notifier) {
    notifier->showNetworkNotification(err);
  }
}

void FuseRequestContext::genericErrorHandler(
    const std::exception& err,
    Notifier* FOLLY_NULLABLE notifier) {
  XLOG(DBG5, folly::exceptionStr(err));
  replyError(EIO);
  if (notifier) {
    notifier->showNetworkNotification(err);
  }
}

void FuseRequestContext::timeoutErrorHandler(
    const folly::FutureTimeout& err,
    Notifier* FOLLY_NULLABLE notifier) {
  XLOGF_EVERY_MS(
      WARN, 1000, "FUSE request timed out: {}", folly::exceptionStr(err));
  replyError(ETIMEDOUT);
  if (notifier) {
    notifier->showNetworkNotification(err);
  }
}

void FuseRequestContext::replyError(int err) {
  XCHECK(err >= 0) << "errno values are positive";
  channel_->replyError(stealReqWithResult(-err), err);
}

void FuseRequestContext::replyNone() {
  stealReqWithResult(0);
}

} // namespace facebook::eden

#endif

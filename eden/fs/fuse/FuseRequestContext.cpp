/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/FuseRequestContext.h"

#include <folly/logging/xlog.h>

#include "eden/fs/notifications/Notifications.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;

namespace facebook::eden {

FuseRequestContext::FuseRequestContext(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader)
    : RequestContext(channel->getProcessAccessLog()),
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
    Notifications* FOLLY_NULLABLE notifications) {
  int errnum = EIO;
  if (isErrnoError(err)) {
    errnum = err.code().value();
  }
  XLOG(DBG5) << folly::exceptionStr(err);
  replyError(errnum);
  if (notifications) {
    notifications->showGenericErrorNotification(err);
  }
}

void FuseRequestContext::genericErrorHandler(
    const std::exception& err,
    Notifications* FOLLY_NULLABLE notifications) {
  XLOG(DBG5) << folly::exceptionStr(err);
  replyError(EIO);
  if (notifications) {
    notifications->showGenericErrorNotification(err);
  }
}

void FuseRequestContext::timeoutErrorHandler(
    const folly::FutureTimeout& err,
    Notifications* FOLLY_NULLABLE notifications) {
  XLOG_EVERY_MS(WARN, 1000)
      << "FUSE request timed out: " << folly::exceptionStr(err);
  replyError(ETIMEDOUT);
  if (notifications) {
    notifications->showGenericErrorNotification(err);
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

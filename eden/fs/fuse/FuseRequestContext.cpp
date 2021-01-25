/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

namespace facebook {
namespace eden {

FuseRequestContext::FuseRequestContext(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader)
    : RequestContext(channel->getProcessAccessLog()),
      channel_(channel),
      fuseHeader_(fuseHeader) {}

fuse_in_header FuseRequestContext::stealReq() {
  if (fuseHeader_.opcode == 0) {
    throw std::runtime_error("req_ has been released");
  }
  fuse_in_header res = fuseHeader_;
  fuseHeader_.opcode = 0;
  return res;
}

const fuse_in_header& FuseRequestContext::getReq() const {
  if (fuseHeader_.opcode == 0) {
    throw std::runtime_error("req_ has been released");
  }
  return fuseHeader_;
}

const fuse_in_header& FuseRequestContext::examineReq() const {
  // Will just return the fuseHeader_ and not throw(unlike getReq)
  // The caller is responsible to check the opcode and ignore if zero
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
  error_ = err;
  channel_->replyError(stealReq(), err);
}

void FuseRequestContext::replyNone() {
  error_ = 0;
  stealReq();
}

} // namespace eden
} // namespace facebook

#endif

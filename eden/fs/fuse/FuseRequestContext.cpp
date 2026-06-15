/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/FuseRequestContext.h"

#include <cerrno>

#include <folly/logging/xlog.h>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/utils/SystemError.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/notifications/Notifier.h"
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include "eden/fs/telemetry/ErrorLogger.h"

using namespace folly;

namespace facebook::eden {

namespace {
// Returns true if a FUSE errno is an unexpected error worth logging to
// perfpipe_edenfs_errors.
constexpr bool shouldLogFuseError(int errnum) {
  return errnum == EIO || errnum == ENOSPC || errnum == EROFS ||
      errnum == EDQUOT || errnum == ENOMEM || errnum == ENFILE ||
      errnum == EMFILE;
}
} // namespace

FuseRequestContext::FuseRequestContext(
    FuseChannel* channel,
    const fuse_in_header& fuseHeader)
    : RequestContext(
          channel->getProcessAccessLog(),
          channel->getEdenFsEventsLogger(),
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
  if (shouldLogFuseError(errnum)) {
    channel_->getErrorLogger().log(
        EdenErrorInfo::fuse(
            ErrorArg::fromExceptionWithoutTrace(err),
            fuseHeader_.nodeid,
            channel_->getMountPath())
            .withErrorCode(errnum));
  }
  replyError(errnum);
  if (notifier) {
    notifier->showNetworkNotification(err);
  }
}

void FuseRequestContext::genericErrorHandler(
    const std::exception& err,
    Notifier* FOLLY_NULLABLE notifier) {
  XLOG(DBG5, folly::exceptionStr(err));
  channel_->getErrorLogger().log(
      EdenErrorInfo::fuse(
          ErrorArg::fromExceptionWithoutTrace(err),
          fuseHeader_.nodeid,
          channel_->getMountPath()));
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

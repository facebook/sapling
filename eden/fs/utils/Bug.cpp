/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/Bug.h"

#include <folly/Conv.h>
#include <folly/ExceptionWrapper.h>
#include <folly/logging/xlog.h>

namespace {
static std::atomic<int> edenBugDisabledCount{0};
}

namespace facebook {
namespace eden {
EdenBug::EdenBug(const char* file, int lineNumber)
    : file_(file), lineNumber_(lineNumber), message_("!!BUG!! ") {}

EdenBug::EdenBug(EdenBug&& other) noexcept
    : file_(other.file_),
      lineNumber_(other.lineNumber_),
      message_(std::move(other.message_)) {
  other.throwOnDestruction_ = false;
}

EdenBug::~EdenBug() noexcept(false) {
  // If toException() has not been called, throw an exception on destruction.
  //
  // Throwing in a destructor is normally poor form, in case we were triggered
  // by stack unwinding of another exception.  However our callers should
  // always use EdenBug objects as temporaries when they want the EDEN_BUG()
  // macro to throw directly.  Therefore we shouldn't have been triggered
  // during stack unwinding of another exception.
  //
  // Callers should only ever store EdenBug objects if they plan to call
  // toException() on them.
  if (throwOnDestruction_) {
    throwException();
  }
}

folly::exception_wrapper EdenBug::toException() {
  logError();
  throwOnDestruction_ = false;
  return folly::exception_wrapper(std::runtime_error(message_));
}

void EdenBug::throwException() {
  toException().throw_exception();
}

void EdenBug::logError() {
  XLOG(CRITICAL) << "EDEN_BUG at " << file_ << ":" << lineNumber_ << ": "
                 << message_;

#ifndef NDEBUG
  // Crash in debug builds.
  // However, allow test code to disable crashing so that we can exercise
  // EDEN_BUG() code paths in tests.
  if (edenBugDisabledCount.load() == 0) {
    XLOG(FATAL) << "crashing due to EDEN_BUG";
  }
#endif
}

EdenBugDisabler::EdenBugDisabler() {
  ++edenBugDisabledCount;
}

EdenBugDisabler::~EdenBugDisabler() {
  --edenBugDisabledCount;
}
} // namespace eden
} // namespace facebook

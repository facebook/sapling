/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenError.h"
#include "eden/fs/utils/SystemError.h"
#ifdef _WIN32
#include "eden/fs/win/utils/WinError.h" // @manual
#endif

namespace facebook {
namespace eden {

EdenError newEdenError(const std::system_error& ex) {
  if (isErrnoError(ex)) {
    return newEdenError(
        ex.code().value(), EdenErrorType::POSIX_ERROR, ex.what());
  }
#ifdef _WIN32
  else if (dynamic_cast<const Win32ErrorCategory*>(&ex.code().category())) {
    return newEdenError(
        ex.code().value(), EdenErrorType::WIN32_ERROR, ex.what());
  } else if (dynamic_cast<const HResultErrorCategory*>(&ex.code().category())) {
    return newEdenError(
        ex.code().value(), EdenErrorType::HRESULT_ERROR, ex.what());
  }
#endif
  else {
    return newEdenError(EdenErrorType::GENERIC_ERROR, ex.what());
  }
}

EdenError newEdenError(const std::exception& ex) {
  const auto* edenError = dynamic_cast<const EdenError*>(&ex);
  if (edenError) {
    return *edenError;
  }
  const auto* systemError = dynamic_cast<const std::system_error*>(&ex);
  if (systemError) {
    return newEdenError(*systemError);
  }
  return newEdenError(
      EdenErrorType::GENERIC_ERROR, folly::exceptionStr(ex).toStdString());
}

EdenError newEdenError(const folly::exception_wrapper& ew) {
  EdenError err;
  if (!ew.with_exception([&err](const EdenError& ex) { err = ex; }) &&
      !ew.with_exception(
          [&err](const std::system_error& ex) { err = newEdenError(ex); })) {
    err = newEdenError(
        EdenErrorType::GENERIC_ERROR, folly::exceptionStr(ew).toStdString());
  }
  return err;
}
} // namespace eden
} // namespace facebook

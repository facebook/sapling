/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/EdenError.h"
#include <re2/re2.h>

#include "eden/common/utils/SystemError.h"
#include "eden/common/utils/windows/WinError.h"

namespace facebook::eden {

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
          [&err](const std::system_error& ex) { err = newEdenError(ex); }) &&
      !ew.with_exception([&err](const sapling::SaplingBackingStoreError& ex) {
        err = newEdenError(ex);
      })) {
    err = newEdenError(
        EdenErrorType::GENERIC_ERROR, folly::exceptionStr(ew).toStdString());
  }
  return err;
}

namespace {
// TODO: Stop relying on formats of Sapling error messages. Instead,
// pass errors with formally defined structures across backingstore FFI.
std::optional<int> extractNetworkError(std::string_view msg) {
  static const re2::RE2 kCurl{R"(Network Error: \[(\d+)\])"};
  static const re2::RE2 kHttp{R"(server responded (\d+))"};
  int code;
  if (RE2::PartialMatch(msg, kCurl, &code) ||
      RE2::PartialMatch(msg, kHttp, &code)) {
    return code;
  }
  return std::nullopt;
}
} // namespace

EdenError newEdenError(const sapling::SaplingBackingStoreError& ex) {
  std::optional<int> code = extractNetworkError(ex.what());
  if (code.has_value()) {
    return newEdenError(
        code.value(),
        EdenErrorType::NETWORK_ERROR,
        folly::exceptionStr(ex).toStdString());
  }

  return newEdenError(static_cast<const std::exception&>(ex));
}

} // namespace facebook::eden

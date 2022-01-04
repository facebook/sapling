/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#ifdef _WIN32

#include <folly/Try.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Windows.h>
#include <string>
#include <system_error>

namespace facebook {
namespace eden {

class Win32ErrorCategory : public std::error_category {
 public:
  const char* name() const noexcept override;
  std::string message(int error) const override;
  static const std::error_category& get() noexcept;
};

class HResultErrorCategory : public std::error_category {
 public:
  const char* name() const noexcept override;
  std::string message(int error) const override;
  static const std::error_category& get() noexcept;
};

//
// Helper function to build and throw the system error from Win32 and HResult
//

inline std::system_error makeHResultErrorExplicit(
    HRESULT code,
    const std::string& description) {
  return std::system_error(code, HResultErrorCategory::get(), description);
}

[[noreturn]] inline void throwHResultErrorExplicit(
    HRESULT code,
    const std::string& description) {
  throw makeHResultErrorExplicit(code, description);
}

inline std::system_error makeWin32ErrorExplicit(
    DWORD code,
    const std::string& description) {
  return std::system_error(code, Win32ErrorCategory::get(), description);
}

[[noreturn]] inline void throwWin32ErrorExplicit(
    DWORD code,
    const std::string& description) {
  throw makeWin32ErrorExplicit(code, description);
}

std::string win32ErrorToString(uint32_t error);

//
// exceptionToHResult is called inside a catch. It will try to return an
// appropriate HRESULT code for the exception. again and catch the right
//
HRESULT exceptionToHResult(const std::exception& ex) noexcept;

template <typename T>
HRESULT tryToHResult(const folly::Try<T>& try_) noexcept {
  if (try_.hasValue()) {
    return S_OK;
  } else {
    auto* exc = try_.tryGetExceptionObject();
    return exceptionToHResult(*exc);
  }
}

// This function can take a function with no args and run it under a try catch
// block. It will catch the exception and return a HRESULT for that. Use a
// lambda if you need to pass args.
//
template <typename Callable>
static HRESULT exceptionToHResultWrapper(Callable&& f) noexcept {
  try {
    return f();
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

} // namespace eden
} // namespace facebook
#endif

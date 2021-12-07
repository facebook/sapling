/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32
#include "eden/fs/utils/WinError.h"
#include <iostream>
#include <memory>
#include <sstream>
namespace facebook {
namespace eden {

std::string win32ErrorToString(uint32_t error) {
  struct LocalDeleter {
    void operator()(HLOCAL p) noexcept {
      ::LocalFree(p);
    }
  };

  LPSTR messageBufferRaw = nullptr;
  // By default, FormatMessageA will terminate the string with "\r\n",
  // and the mis-named (and mis-documented) FORMAT_MESSAGE_MAX_WIDTH_MASK flag
  // will remove these (but add a whitespace instead).
  size_t size = FormatMessageA(
      FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
          FORMAT_MESSAGE_IGNORE_INSERTS | FORMAT_MESSAGE_MAX_WIDTH_MASK,
      nullptr,
      error,
      MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
      (LPSTR)&messageBufferRaw,
      0,
      nullptr);
  // Get a unique_ptr to the raw buffer, so it's released in case of an
  // exception.
  std::unique_ptr<char, LocalDeleter> messageBuffer{messageBufferRaw};

  std::stringstream stream;
  if ((size > 0) && (messageBuffer)) {
    stream << "Error (0x" << std::hex << error << ") " << messageBuffer.get();
  } else {
    stream << "Error (0x" << std::hex << error << ") Unknown Error";
  }
  return stream.str();
}

const char* Win32ErrorCategory::name() const noexcept {
  return "Win32 Error";
}

std::string Win32ErrorCategory::message(int error) const {
  return win32ErrorToString(error);
}

const std::error_category& Win32ErrorCategory::get() noexcept {
  static class Win32ErrorCategory cat;
  return cat;
}

const char* HResultErrorCategory::name() const noexcept {
  return "HRESULT Error";
}

std::string HResultErrorCategory::message(int error) const {
  return win32ErrorToString(error);
}

const std::error_category& HResultErrorCategory::get() noexcept {
  static class HResultErrorCategory cat;
  return cat;
}

HRESULT exceptionToHResult(const std::exception& ex) noexcept {
  XLOG(ERR) << folly::exceptionStr(ex);
  if (auto e = dynamic_cast<const std::system_error*>(&ex)) {
    auto code = e->code();
    if (code.category() == HResultErrorCategory::get()) {
      return code.value();
    }
    if (code.category() == Win32ErrorCategory::get()) {
      return HRESULT_FROM_WIN32(code.value());
    }
    return HRESULT_FROM_WIN32(ERROR_ERRORS_ENCOUNTERED);
  } else if (auto e = dynamic_cast<const std::bad_alloc*>(&ex)) {
    return E_OUTOFMEMORY;
  } else {
    return HRESULT_FROM_WIN32(ERROR_ERRORS_ENCOUNTERED);
  }
}

} // namespace eden
} // namespace facebook
#endif

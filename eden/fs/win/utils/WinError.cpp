/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "WinError.h"
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
  size_t size = FormatMessageA(
      FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
          FORMAT_MESSAGE_IGNORE_INSERTS,
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
    stream << "Error (0x" << std::hex << error << ") Unknown Error\r\n";
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

HRESULT exceptionToHResult() noexcept {
  try {
    throw;
  } catch (const std::system_error& ex) {
    auto code = ex.code();
    XLOG(ERR) << ex.what() << " : " << ex.code();
    if (code.category() == HResultErrorCategory::get()) {
      return code.value();
    }
    if (code.category() == Win32ErrorCategory::get()) {
      return HRESULT_FROM_WIN32(code.value());
    }
    return ERROR_ERRORS_ENCOUNTERED;

  } catch (std::bad_alloc const&) {
    return E_OUTOFMEMORY;

  } catch (const std::exception& ex) {
    XLOG(ERR) << ex.what();
    return ERROR_ERRORS_ENCOUNTERED;

  } catch (...) {
    // Make sure not to leak any exception out of here. I don't think we will
    // hit this though. Break in debug build if we do.
#ifndef NDEBUG
    DebugBreak();
#endif
    return ERROR_ERRORS_ENCOUNTERED;
  }
}

} // namespace eden
} // namespace facebook

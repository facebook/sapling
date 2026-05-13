/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ThrowTraceCapture.h"

#include <cstdlib>

#ifdef __APPLE__
#include "folly/debugging/exception_tracer/ExceptionTracerLib.h"
#endif
#ifdef _WIN32
#include <windows.h>
#endif

#include "eden/fs/rust/backtrace_ffi/src/lib.rs.h"

namespace {
void onThrow() {
  constexpr size_t kMaxStackDepth = 64;
  facebook::eden::capture_backtrace(kMaxStackDepth);
}
} // namespace

#if defined(__linux__)
// Linux throw hook using the GNU linker's --wrap mechanism.
//
// When -Wl,--wrap=__cxa_throw is passed (via exported_linker_flags in BUCK),
// the linker redirects all calls to __cxa_throw to __wrap___cxa_throw, and
// makes the original available as __real___cxa_throw. This lets us intercept
// every C++ throw to capture a backtrace before the stack unwinds.
//
// Flow: throw expr → __wrap___cxa_throw → onThrow() → capture_backtrace()
//       → __real___cxa_throw (original, performs the actual throw)

extern "C" {
void __real___cxa_throw(void*, void*, void (*)(void*))
    __attribute__((__noreturn__));

__attribute__((__noreturn__)) void __wrap___cxa_throw(
    void* thrownException,
    void* type,
    void (*destructor)(void*)) {
  onThrow();
  __real___cxa_throw(thrownException, type, destructor);
  __builtin_unreachable();
}
} // extern "C"

#elif defined(__APPLE__)
// macOS: Register with folly's exception tracer callback system

static void throwCallback(void*, std::type_info*, void (**)(void*)) noexcept {
  onThrow();
}

static struct RegisterThrowCallback {
  RegisterThrowCallback() {
    folly::exception_tracer::registerCxaThrowCallback(throwCallback);
  }
} registrar;

#elif defined(_WIN32)
// Windows: Vectored Exception Handler catches C++ exceptions (SEH 0xE06D7363).

constexpr DWORD kCppExceptionCode = 0xE06D7363;

LONG WINAPI cppExceptionHandler(PEXCEPTION_POINTERS info) {
  if (info && info->ExceptionRecord &&
      info->ExceptionRecord->ExceptionCode == kCppExceptionCode) {
    onThrow();
  }
  return EXCEPTION_CONTINUE_SEARCH;
}

// AddVectoredExceptionHandler first parameter: CALL_FIRST (1) registers our
// handler at the front of the VEH list so it runs before other handlers,
// ensuring we capture the stack trace before the exception is modified.
// See:
// https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-addvectoredexceptionhandler
#define CALL_FIRST 1

struct VehRegistrar {
  PVOID handle_ = nullptr;
  VehRegistrar(const VehRegistrar&) = delete;
  VehRegistrar& operator=(const VehRegistrar&) = delete;
  VehRegistrar(VehRegistrar&&) = delete;
  VehRegistrar& operator=(VehRegistrar&&) = delete;

  VehRegistrar() {
    handle_ = AddVectoredExceptionHandler(CALL_FIRST, cppExceptionHandler);
    if (!handle_) {
      fprintf(
          stderr,
          "Failed to register VEH for throw-site stack trace capture\n");
    }
  }
  ~VehRegistrar() {
    if (handle_) {
      RemoveVectoredExceptionHandler(handle_);
    }
  }
};

static VehRegistrar vehRegistrar;

#endif // platform hooks

namespace facebook::eden {

// Lazy symbolization via Rust FFI — resolves captured frames on demand.
std::optional<std::string> getThrowSiteStackTrace() {
  auto trace = symbolize_captured_trace();
  if (trace.empty()) {
    return std::nullopt;
  }
  return std::string(trace.data(), trace.size());
}

} // namespace facebook::eden

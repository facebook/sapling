/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/WinStackTrace.h"

#ifdef _WIN32
#include <string>
#include <system_error>

#include <folly/CPortability.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/StringConv.h"
#include "eden/common/utils/windows/WinError.h"

#ifndef OUT
#define OUT
#endif

#ifndef IN
#define IN
#endif
#include <Dbghelp.h> // @manual

namespace facebook::eden {
namespace {
// 4k for a symbol name? Demangled symbols are pretty huge
static constexpr size_t kMaxSymbolLen = 4096;
static constexpr size_t kMaxFrames = 64;
static constexpr size_t kMaxLineLength = kMaxSymbolLen + 128;

int formatString(char* buffer, size_t size, const char* format, ...) {
  va_list args = NULL;
  va_start(args, format);
  int length = FormatMessageA(
      FORMAT_MESSAGE_FROM_STRING, format, 0, 0, buffer, size, &args);
  va_end(args);
  return length;
}

std::optional<AbsolutePath> getBinaryDirectory(HANDLE proc) {
  constexpr size_t kBufferSize = 1024;

  wchar_t buffer[kBufferSize];
  DWORD buffer_size = kBufferSize;

  if (!QueryFullProcessImageNameW(proc, 0, buffer, &buffer_size)) {
    // We can't throw exception in exception handling code, logging it instead.
    XLOGF(
        WARN,
        "Failed to QueryFullProcessImageNameW: {}",
        win32ErrorToString(GetLastError()));
    return std::nullopt;
  }

  auto pathStr = wideToMultibyteString<std::string>(
      std::wstring_view{buffer, buffer_size});
  auto binary = canonicalPath(pathStr);
  return binary.dirname().copy();
}

void setUpSearchPath(HANDLE proc) {
  // Get current configured symbol search path
  wchar_t buffer[1024];
  if (!SymGetSearchPathW(proc, buffer, 1024)) {
    XLOGF(
        WARN,
        "Failed to SymGetSearchPathW: {}",
        win32ErrorToString(GetLastError()));
    return;
  }

  auto size = wcsnlen_s(buffer, 1024);
  std::wstring searchPath{buffer, size};

  // Add the directory containing the binary to the search path
  if (auto parent = getBinaryDirectory(proc)) {
    searchPath += L";";
    searchPath += parent->wide();

    SymSetSearchPathW(proc, searchPath.data());

    XLOGF(
        DBG6,
        "Setting symbol search path to {}",
        wideToMultibyteString<std::string>(searchPath));

    // Force dbghelp to load PDB from the newly updated path
    SymRefreshModuleList(proc);
  }
}

HANDLE initSym() {
  HANDLE proc = GetCurrentProcess();
  SymSetOptions(
      SYMOPT_LOAD_LINES | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS |
      SYMOPT_UNDNAME);
  SymInitializeW(proc, NULL, TRUE);
  try {
    setUpSearchPath(proc);
  } catch (const std::exception& ex) {
    XLOGF(DBG6, "Failed to set up symbol search path: {}", ex.what());
  }
  return proc;
}

HANDLE getSymProc() {
  static HANDLE proc = initSym();
  return proc;
}

FOLLY_NOINLINE size_t backtrace(void** frames, size_t n_frames) {
  // Skip the first two frames; they're always going to show backtrace and
  // windowsExceptionFilter.
  return CaptureStackBackTrace(2, (DWORD)n_frames, frames, NULL);
}

void backtraceSymbols(void** array, size_t n_frames, HANDLE out) {
  union {
    SYMBOL_INFO info;
    char buf[sizeof(SYMBOL_INFO) + kMaxSymbolLen];
  } sym;

  sym.info = SYMBOL_INFO();
  sym.info.SizeOfStruct = sizeof(SYMBOL_INFO);
  sym.info.MaxNameLen = kMaxSymbolLen;

  IMAGEHLP_LINE64 line;
  line.SizeOfStruct = sizeof(line);

  HANDLE proc = getSymProc();
  char output[kMaxLineLength];
  for (size_t i = 0; i < n_frames; i++) {
    DWORD64 addr = (DWORD64)(intptr_t)array[i];
    if (!SymFromAddr(proc, addr, 0, &sym.info)) {
      formatString(
          sym.info.Name,
          sym.info.MaxNameLen,
          "<failed to resolve symbol: %1!s!>\n",
          std::system_category().message(GetLastError()).c_str());
    }

    DWORD displacement;
    if (SymGetLineFromAddr64(proc, addr, &displacement, &line)) {
      int size = formatString(
          output,
          kMaxLineLength,
          "#%1!zu! %2!p! %3!s! %4!s!:%5!lu!\n",
          i,
          array[i],
          sym.info.Name,
          line.FileName,
          line.LineNumber);
      WriteFile(out, output, size, NULL, NULL);
    } else {
      int size = formatString(
          output,
          kMaxLineLength,
          "#%1!zu! %2!p! %3!s!\n",
          i,
          array[i],
          sym.info.Name);
      WriteFile(out, output, size, NULL, NULL);
    }
  }
}

size_t backtraceFromException(
    LPEXCEPTION_POINTERS exception,
    void** frames,
    size_t n_frames) {
  auto context = exception->ContextRecord;
  auto thread = GetCurrentThread();
  STACKFRAME64 frame;
  DWORD image;
#if _M_X64
  image = IMAGE_FILE_MACHINE_AMD64;
  frame.AddrPC.Offset = context->Rip;
  frame.AddrPC.Mode = AddrModeFlat;
  frame.AddrFrame.Offset = context->Rsp;
  frame.AddrFrame.Mode = AddrModeFlat;
  frame.AddrStack.Offset = context->Rsp;
  frame.AddrStack.Mode = AddrModeFlat;
#else
  return 0; // No stack trace for you!
#endif
  HANDLE proc = getSymProc();
  size_t i = 0;
  while (i < n_frames &&
         StackWalk64(
             image,
             proc,
             thread,
             &frame,
             context,
             nullptr,
             nullptr,
             nullptr,
             nullptr)) {
    frames[i++] = (void*)frame.AddrPC.Offset;
  }
  return i;
}

LONG WINAPI windowsExceptionFilter(LPEXCEPTION_POINTERS excep) {
  void* frames[kMaxFrames];
  size_t size = backtraceFromException(excep, frames, kMaxFrames);

  char line[kMaxLineLength];
  HANDLE err = GetStdHandle(STD_ERROR_HANDLE);

  int length = formatString(
      line,
      kMaxLineLength,
      "Unhandled win32 exception code=0x%1!lX!. Fatal error detected at:\n",
      excep->ExceptionRecord->ExceptionCode);
  WriteFile(err, line, length, NULL, NULL);
  for (DWORD i = 0; i < excep->ExceptionRecord->NumberParameters; ++i) {
    length = formatString(
        line,
        kMaxLineLength,
        "  param=0x%1!lx!\n",
        excep->ExceptionRecord->ExceptionInformation[i]);
    WriteFile(err, line, length, NULL, NULL);
  }

  backtraceSymbols(frames, size, err);

  length = formatString(
      line,
      kMaxLineLength,
      "The stacktrace for the exception filter call is:\n");
  WriteFile(err, line, length, NULL, NULL);

  size = backtrace(frames, kMaxFrames);
  backtraceSymbols(frames, size, err);

  // Call an exception that bypass all exception handlers. This will create a
  // crash dump on disk by default.
  SetUnhandledExceptionFilter(nullptr);
  UnhandledExceptionFilter(excep);

  // Terminate the process.
  // msvcrt abort() ultimately calls exit(3), so we shortcut that.
  // Ideally we'd just exit() or ExitProcess() and be done, but it
  // is documented as possible (or even likely!) that deadlock
  // is possible, so we use TerminateProcess() to force ourselves
  // to terminate.
  TerminateProcess(GetCurrentProcess(), 3);
  // However, TerminateProcess() is asynchronous and we will continue
  // running here.  Let's also try exiting normally and see which
  // approach wins!
  _exit(3);
}
} // namespace

void installWindowsExceptionFilter() {
  SetUnhandledExceptionFilter(windowsExceptionFilter);

  // Call `getSymProc` to set up the environment for loading symbols. This way
  // we won't need to load symbol when exception happens but at startup. Less
  // risks.
  getSymProc();
}

void printCurrentStack() {
  void* frames[kMaxFrames];
  size_t size = backtrace(frames, kMaxFrames);
  HANDLE err = GetStdHandle(STD_ERROR_HANDLE);
  backtraceSymbols(frames, size, err);
}
} // namespace facebook::eden
#endif

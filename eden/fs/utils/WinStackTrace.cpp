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

HANDLE initSym() {
  HANDLE proc = GetCurrentProcess();
  SymInitialize(proc, NULL, TRUE);
  SymSetOptions(
      SYMOPT_LOAD_LINES | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS |
      SYMOPT_UNDNAME);
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

  backtraceSymbols(frames, size, err);

  length = formatString(
      line,
      kMaxLineLength,
      "The stacktrace for the exception filter call is:\n");
  WriteFile(err, line, length, NULL, NULL);

  size = backtrace(frames, kMaxFrames);
  backtraceSymbols(frames, size, err);

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
}
} // namespace facebook::eden
#endif

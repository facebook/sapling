/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "Subprocess.h"
#include <fcntl.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <io.h>
#include <iostream>
#include <string>
#include "Pipe.h"
#include "eden/fs/win/Edenwin.h"

namespace facebook {
namespace eden {
using namespace std;

Subprocess::Subprocess() {}
Subprocess::Subprocess(const std::vector<string>& cmd) {
  createSubprocess(cmd);
}

Subprocess::~Subprocess() {}

void Subprocess::createSubprocess(
    const std::vector<string>& cmd,
    const char* currentDir,
    std::unique_ptr<Pipe> childInPipe,
    std::unique_ptr<Pipe> childOutPipe) {
  childInPipe_ = std::move(childInPipe);
  childOutPipe_ = std::move(childOutPipe);

  PROCESS_INFORMATION procInfo;
  STARTUPINFOA startupInfo;
  bool status = FALSE;

  ZeroMemory(&procInfo, sizeof(PROCESS_INFORMATION));
  ZeroMemory(&startupInfo, sizeof(STARTUPINFO));
  startupInfo.cb = sizeof(STARTUPINFO);

  string cmdToProcess;
  for (auto& str : cmd) {
    cmdToProcess += str + " ";
  }

  XLOG(DBG1) << "Creating the process: " << cmdToProcess.c_str() << std::endl;

  status = CreateProcessA(
      nullptr,
      (LPSTR)cmdToProcess.c_str(),
      nullptr,
      nullptr,
      TRUE, // inherit the handles
      0,
      nullptr,
      currentDir,
      &startupInfo,
      &procInfo);

  if (!status) {
    throw std::system_error(
        GetLastError(), std::system_category(), "CreateProcess failed\n");

  } else {
    CloseHandle(procInfo.hProcess);
    CloseHandle(procInfo.hThread);
  }
}
} // namespace eden
} // namespace facebook

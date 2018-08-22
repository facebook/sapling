/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "Edenwin.h"
#include "Subprocess.h"
#include <fcntl.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <io.h>
#include <iostream>
#include <string>
#include "Pipe.h"

using namespace facebook::edenwin;
using namespace std;

Subprocess::Subprocess() {}
Subprocess::Subprocess(const std::vector<string>& cmd) {
  createSubprocess(cmd);
}

Subprocess::~Subprocess() {}

void Subprocess::createSubprocess(
    const std::vector<string>& cmd,
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
      NULL,
      (LPSTR)cmdToProcess.c_str(),
      NULL,
      NULL,
      TRUE, // inherit the handles
      0,
      NULL,
      NULL,
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

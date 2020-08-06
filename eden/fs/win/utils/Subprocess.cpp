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

  if (!SetHandleInformation(
          childInPipe_->readHandle(),
          HANDLE_FLAG_INHERIT,
          HANDLE_FLAG_INHERIT)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "SetHandleInformation failed");
  }
  if (!SetHandleInformation(
          childOutPipe_->writeHandle(),
          HANDLE_FLAG_INHERIT,
          HANDLE_FLAG_INHERIT)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "SetHandleInformation failed");
  }

  HANDLE handles[2] = {childInPipe_->readHandle(),
                       childOutPipe_->writeHandle()};

  PROCESS_INFORMATION procInfo{};
  STARTUPINFOEXA startupInfo{};
  bool status = FALSE;

  startupInfo.StartupInfo.cb = sizeof(STARTUPINFOEXA);

  SIZE_T size;
  InitializeProcThreadAttributeList(nullptr, 1, 0, &size);

  startupInfo.lpAttributeList = (LPPROC_THREAD_ATTRIBUTE_LIST)malloc(size);
  if (startupInfo.lpAttributeList == nullptr) {
    throw std::bad_alloc();
  }

  SCOPE_EXIT {
    free(startupInfo.lpAttributeList);
  };

  if (!InitializeProcThreadAttributeList(
          startupInfo.lpAttributeList, 1, 0, &size)) {
    throw std::system_error(
        GetLastError(),
        std::system_category(),
        "InitializeProcThreadAttributeList failed");
  }

  SCOPE_EXIT {
    DeleteProcThreadAttributeList(startupInfo.lpAttributeList);
  };

  if (!UpdateProcThreadAttribute(
          startupInfo.lpAttributeList,
          0,
          PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
          &handles,
          sizeof(handles),
          nullptr,
          nullptr)) {
    throw std::system_error(
        GetLastError(),
        std::system_category(),
        "UpdateProcThreadAttribute failed");
  }

  string cmdToProcess;
  for (auto& str : cmd) {
    cmdToProcess += str + " ";
  }

  XLOG(DBG1) << "Creating the process: " << cmdToProcess.c_str();

  status = CreateProcessA(
      nullptr,
      (LPSTR)cmdToProcess.c_str(),
      nullptr,
      nullptr,
      TRUE, // inherit the handles
      EXTENDED_STARTUPINFO_PRESENT,
      nullptr,
      currentDir,
      &startupInfo.StartupInfo,
      &procInfo);

  if (!status) {
    throw std::system_error(
        GetLastError(), std::system_category(), "CreateProcess failed\n");

  } else {
    // Close the Pipe handles that are inherited in the child and are not needed
    // in the parent process. This will also make sure the pipe is closed when
    // the child process ends.
    childInPipe_->closeReadHandle();
    childOutPipe_->closeWriteHandle();

    CloseHandle(procInfo.hProcess);
    CloseHandle(procInfo.hThread);
  }
}
} // namespace eden
} // namespace facebook

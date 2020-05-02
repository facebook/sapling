/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/portability/Windows.h>

#include <optional>
#include "eden/fs/service/EdenServer.h"

namespace facebook {
namespace eden {

class WinService {
 public:
  WinService() = default;
  ~WinService() = default;

  WinService(const WinService&) = delete;
  WinService(WinService&&) = delete;

  WinService& operator=(const WinService&) = delete;
  WinService& operator=(WinService&&) = delete;

  /**
   * This function will create the dispatch table and launch the Edenfs service.
   * It will only return either on error or when the service exits. The error
   * log if the service failed to start will be in the edenstartup.log
   */
  static void create(int argc, char** argv);

  /**
   * This is the main function for the Edenfs service.
   */
  static void WINAPI main(DWORD argc, LPSTR* argv);

 private:
  int serviceMain(int argc, char** argv);

  int setup(int argc, char** argv);
  void run();
  void stop();

  static void WINAPI ctrlHandler(DWORD dwCtrl);
  void reportStatus(DWORD currentState, DWORD exitCode, DWORD waitHint);

  SERVICE_STATUS status_;
  SERVICE_STATUS_HANDLE handle_;

  std::optional<EdenServer> server_;
  DWORD dwCheckPoint_{1};
};

} // namespace eden
} // namespace facebook

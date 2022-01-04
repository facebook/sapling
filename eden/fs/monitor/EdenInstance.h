/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/types.h>
#include <array>
#include <chrono>
#include <cstddef>
#include <memory>

#include <folly/File.h>
#include <folly/Portability.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventHandler.h>

#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"

namespace folly {
class EventBase;
template <typename T>
class Future;
struct Unit;
} // namespace folly

namespace facebook {
namespace eden {

class EdenMonitor;
class LogFile;

/**
 * EdenInstance represents a single instance of the edenfs process.
 *
 * It exists to manage the process and inform the EdenMonitor when the edenfs
 * process exits.
 */
class EdenInstance {
 public:
  explicit EdenInstance(EdenMonitor* monitor);
  virtual ~EdenInstance();

  FOLLY_NODISCARD virtual folly::Future<folly::Unit> start() = 0;
  virtual pid_t getPid() const = 0;
  virtual void checkLiveness() = 0;

 protected:
  EdenInstance(EdenInstance const&) = delete;
  EdenInstance& operator=(EdenInstance const&) = delete;

  /*
   * We store a raw pointer to the EdenMonitor.
   * The EdenMonitor owns us, and will destroy us before it is destroyed.
   */
  EdenMonitor* const monitor_;
};

/**
 * ExistingEdenInstance tracks an edenfs process that was not started by this
 * process.
 */
class ExistingEdenInstance : public EdenInstance, private folly::AsyncTimeout {
 public:
  ExistingEdenInstance(EdenMonitor* monitor, pid_t pid);

  FOLLY_NODISCARD folly::Future<folly::Unit> start() override;

  pid_t getPid() const override {
    return pid_;
  }

  void checkLiveness() override;

 private:
  void timeoutExpired() noexcept override;
  bool isAlive();

  pid_t pid_{0};
  std::chrono::milliseconds pollInterval_{60000};
};

/**
 * SpawnedEdenInstance tracks an edenfs process that was spawned directly by
 * this process.
 *
 * It reads stdout and stderr output from EdenFS and writes them to a log file,
 * performing log rotation as necessary.
 */
class SpawnedEdenInstance : public EdenInstance,
                            private folly::EventHandler,
                            private folly::AsyncTimeout {
 public:
  SpawnedEdenInstance(EdenMonitor* monitor, std::shared_ptr<LogFile> log);
  ~SpawnedEdenInstance() override;

  FOLLY_NODISCARD folly::Future<folly::Unit> start() override;

  void takeover(pid_t pid, int logFD);

  pid_t getPid() const override {
    return pid_;
  }

  int getLogPipeFD() const {
    return logPipe_.fd();
  }

  void checkLiveness() override;

 private:
  static constexpr size_t kLogBufferSize = 64 * 1024;

  class StartupStatusChecker;

  void handlerReady(uint16_t events) noexcept override;
  void timeoutExpired() noexcept override;

  void beginProcessingLogPipe();
  void forwardLogOutput();
  void closeLogPipe();
  void checkLivenessImpl();

  AbsolutePath edenfsExe_;
  SpawnedProcess cmd_;
  pid_t pid_{0};
  FileDescriptor logPipe_;
  std::shared_ptr<LogFile> log_;
  std::unique_ptr<StartupStatusChecker> startupChecker_;

  // EdenInstance objects are always allocated on the heap, so we just
  // keep the log buffer in an inline array, rather than in a separately
  // allocated buffer.
  std::array<std::byte, kLogBufferSize> logBuffer_{};
};

} // namespace eden
} // namespace facebook

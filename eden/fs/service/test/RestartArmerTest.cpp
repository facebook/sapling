/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/RestartArmer.h"

#ifdef __APPLE__
#include <sys/stat.h>
#endif

#include <chrono>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

#include <folly/FileUtil.h>
#include <folly/futures/Future.h>
#include <folly/json/json.h>
#include <folly/portability/Unistd.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/privhelper/PrivHelper.h"

using namespace facebook::eden;

namespace {

[[noreturn]] void notImplemented() {
  throw std::logic_error("not implemented by RecordingPrivHelper");
}

/**
 * Records the restart args it is handed, and answers with whatever result the
 * test asked for.
 */
class RecordingPrivHelper final : public PrivHelper {
 public:
  std::vector<EdenFsRestartArgs> restartArgs;
  folly::exception_wrapper setRestartArgsError;
  /** When set, requests stay pending until the test completes this. */
  std::optional<folly::Promise<folly::Unit>> pendingRequest;

  folly::Future<folly::Unit> setRestartArgs(
      const EdenFsRestartArgs& args) override {
    restartArgs.push_back(args);
    if (pendingRequest) {
      return pendingRequest->getFuture();
    }
    if (setRestartArgsError) {
      return folly::makeFuture<folly::Unit>(setRestartArgsError);
    }
    return folly::makeFuture();
  }

  void attachEventBase(folly::EventBase*) override {}
  void detachEventBase() override {}
  folly::Future<folly::File>
  fuseMount(folly::StringPiece, bool, folly::StringPiece) override {
    notImplemented();
  }
  folly::Future<folly::Unit> nfsMount(
      folly::StringPiece,
      const NFSMountOptions&) override {
    notImplemented();
  }
  folly::Future<folly::Unit> fuseUnmount(
      folly::StringPiece,
      const UnmountOptions&) override {
    notImplemented();
  }
  folly::Future<folly::Unit> nfsUnmount(folly::StringPiece) override {
    notImplemented();
  }
  folly::Future<folly::Unit> bindMount(folly::StringPiece, folly::StringPiece)
      override {
    notImplemented();
  }
  folly::Future<folly::Unit> bindUnMount(folly::StringPiece) override {
    notImplemented();
  }
  folly::Future<folly::Unit> takeoverShutdown(folly::StringPiece) override {
    notImplemented();
  }
  folly::Future<folly::Unit> takeoverStartup(
      folly::StringPiece,
      const std::vector<std::string>&) override {
    notImplemented();
  }
  folly::Future<folly::Unit> setLogFile(folly::File) override {
    notImplemented();
  }
  folly::Future<pid_t> getServerPid() override {
    notImplemented();
  }
  folly::Future<NamespaceInfo> getNamespaceInfo(pid_t) override {
    notImplemented();
  }
  folly::Future<pid_t> startFam(
      const std::vector<std::string>&,
      const std::string&,
      const std::string&,
      const bool) override {
    notImplemented();
  }
  folly::Future<StopFileAccessMonitorResponse> stopFam() override {
    notImplemented();
  }
  folly::Future<folly::Unit> setMemoryPriorityForProcess(pid_t, int) override {
    notImplemented();
  }
  folly::Future<folly::Unit> setFuseReadAhead(folly::StringPiece, uint32_t)
      override {
    notImplemented();
  }
  int stop() override {
    return 0;
  }
  int getRawClientFd() const override {
    notImplemented();
  }
  bool checkConnection() override {
    return true;
  }
  int getPid() override {
    return -1;
  }
};

class RestartArmerTest : public ::testing::Test {
 protected:
  void SetUp() override {
    const auto stateDir = canonicalPath(tmpDir_.path().string());
    daemonArgsPath_ = stateDir + ".edenfs_start_args"_pc;
    sentinelPath_ = stateDir + ".edenfs_restart_armed"_pc;
    edenConfig_ = EdenConfig::createTestEdenConfig();
    edenConfig_->restartEdenfsOnCrash.setValue(
        true, ConfigSourceType::CommandLine);
  }

  RestartArmer makeArmer() {
    return RestartArmer{
        &privHelper_,
        std::make_shared<ReloadableConfig>(edenConfig_),
        daemonArgsPath_,
        sentinelPath_};
  }

  static bool exists(const AbsolutePath& path) {
    return ::access(path.c_str(), F_OK) == 0;
  }

  folly::test::TemporaryDirectory tmpDir_;
  AbsolutePath daemonArgsPath_;
  AbsolutePath sentinelPath_;
  std::shared_ptr<EdenConfig> edenConfig_;
  RecordingPrivHelper privHelper_;
};

TEST_F(RestartArmerTest, aFreshArmerIsDisarmed) {
  EXPECT_FALSE(makeArmer().armed());
}

TEST_F(RestartArmerTest, removingTheSentinelIsIdempotent) {
  ASSERT_TRUE(folly::writeFile(std::string{"{}"}, sentinelPath_.c_str()));
  auto armer = makeArmer();

  armer.removeSentinel();
  ASSERT_FALSE(exists(sentinelPath_));

  armer.removeSentinel();
  EXPECT_FALSE(exists(sentinelPath_));
}

#ifdef __APPLE__

constexpr folly::StringPiece kDaemonArgs{
    R"({"restart_cmd": ["/usr/local/bin/edenfs", "--edenDir", "/tmp/eden"],
        "env": {"PATH": "/usr/bin", "HOME": "/home/eden"}})"};

/** Sets an environment variable for the duration of one test. */
class ScopedEnvVar {
 public:
  ScopedEnvVar(folly::StringPiece name, const char* value) : name_{name.str()} {
    ::setenv(name_.c_str(), value, 1);
  }
  ~ScopedEnvVar() {
    ::unsetenv(name_.c_str());
  }
  ScopedEnvVar(const ScopedEnvVar&) = delete;
  ScopedEnvVar& operator=(const ScopedEnvVar&) = delete;

 private:
  std::string name_;
};

folly::dynamic readJson(const AbsolutePath& path) {
  std::string contents;
  if (!folly::readFile(path.c_str(), contents)) {
    throw std::runtime_error("cannot read " + path.asString());
  }
  return folly::parseJson(contents);
}

TEST_F(RestartArmerTest, armingWritesTheSentinel) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  makeArmer().arm();

  struct stat st{};
  ASSERT_EQ(0, ::stat(sentinelPath_.c_str(), &st));
  EXPECT_EQ(0600, st.st_mode & 07777);

  const auto sentinel = readJson(sentinelPath_);
  EXPECT_EQ(
      folly::dynamic::array("/usr/local/bin/edenfs", "--edenDir", "/tmp/eden"),
      sentinel["argv"]);
  EXPECT_EQ(
      folly::dynamic{
          folly::dynamic::object("PATH", "/usr/bin")("HOME", "/home/eden")},
      sentinel["env"]);
  EXPECT_NE(0, sentinel["nonce"].asInt());
}

TEST_F(RestartArmerTest, armingSendsTheRequestAndMarksItselfArmed) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  auto armer = makeArmer();
  armer.arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  const auto& args = privHelper_.restartArgs.front();
  EXPECT_TRUE(args.enabled);
  EXPECT_EQ(sentinelPath_.asString(), args.sentinelPath);
  EXPECT_EQ(
      readJson(sentinelPath_)["nonce"].asInt(),
      static_cast<int64_t>(args.sentinelNonce));
  EXPECT_TRUE(armer.armed());
}

TEST_F(RestartArmerTest, aRejectedRequestRemovesTheSentinelAgain) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  privHelper_.setRestartArgsError =
      folly::make_exception_wrapper<std::runtime_error>("unknown request");

  auto armer = makeArmer();
  armer.arm();

  EXPECT_FALSE(armer.armed());
  EXPECT_FALSE(exists(sentinelPath_));
}

TEST_F(RestartArmerTest, aMissingDaemonArgsFileDoesNotArm) {
  auto armer = makeArmer();
  armer.arm();

  EXPECT_FALSE(armer.armed());
  EXPECT_FALSE(exists(sentinelPath_));
  EXPECT_TRUE(privHelper_.restartArgs.empty());
}

TEST_F(RestartArmerTest, anEmptyEnvironmentDoesNotArm) {
  ASSERT_TRUE(
      folly::writeFile(
          std::string{
              R"({"restart_cmd": ["/usr/local/bin/edenfs"], "env": {}})"},
          daemonArgsPath_.c_str()));

  auto armer = makeArmer();
  armer.arm();

  EXPECT_FALSE(armer.armed());
  EXPECT_FALSE(exists(sentinelPath_));
  EXPECT_TRUE(privHelper_.restartArgs.empty());
}

// Runs the rejection path after the armer is gone. Reaching destroyed storage
// is only reported under a sanitizer, so the assertion below can pass either
// way; what this pins down is that the path is exercised at all.
TEST_F(RestartArmerTest, destroyingTheArmerWithARequestInFlight) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  privHelper_.pendingRequest.emplace();

  {
    auto armer = makeArmer();
    armer.arm();
    ASSERT_EQ(1, privHelper_.restartArgs.size());
    ASSERT_TRUE(exists(sentinelPath_));
  }

  privHelper_.pendingRequest->setException(
      std::runtime_error("privhelper went away"));

  EXPECT_FALSE(exists(sentinelPath_));
}

TEST_F(RestartArmerTest, theRestartBudgetIsCarriedOverFromTheEnvironment) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  const ScopedEnvVar count{kEdenFsRestartCountEnv, "2"};
  const ScopedEnvVar firstAt{kEdenFsFirstRestartAtEnv, "1700000000"};
  edenConfig_->restartEdenfsMaxCount.setValue(5, ConfigSourceType::CommandLine);
  edenConfig_->restartEdenfsWindow.setValue(
      std::chrono::nanoseconds{std::chrono::seconds{90}},
      ConfigSourceType::CommandLine);

  makeArmer().arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  const auto& args = privHelper_.restartArgs.front();
  EXPECT_EQ(2, args.restartCount);
  EXPECT_EQ(1700000000, args.firstRestartEpochSec);
  EXPECT_EQ(5, args.maxRestarts);
  EXPECT_EQ(90, args.windowSeconds);
}

#endif // __APPLE__

} // namespace

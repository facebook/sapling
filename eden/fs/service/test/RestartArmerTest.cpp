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

#include <algorithm>
#include <chrono>
#include <filesystem>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <folly/FileUtil.h>
#include <folly/futures/Future.h>
#include <folly/portability/Unistd.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/service/EdenStateDir.h"

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
  RestartArmerTest()
      : stateDirPath_{canonicalPath(tmpDir_.path().string())},
        stateDir_{stateDirPath_},
        daemonArgsPath_{stateDir_.getDaemonArgsPath()},
        edenConfig_{EdenConfig::createTestEdenConfig()} {
    edenConfig_->restartEdenfsOnCrash.setValue(
        true, ConfigSourceType::CommandLine);
  }

  RestartArmer makeArmer() {
    return RestartArmer{
        &privHelper_,
        std::make_shared<ReloadableConfig>(edenConfig_),
        stateDir_};
  }

  static bool exists(const AbsolutePath& path) {
    return ::access(path.c_str(), F_OK) == 0;
  }

#ifdef __APPLE__
  /** Where the most recent arm told the privhelper it had put the sentinel. */
  AbsolutePath armedSentinelPath() const {
    return canonicalPath(privHelper_.restartArgs.back().sentinelPath);
  }

  /** Every name in the state directory, sorted. */
  std::vector<std::string> stateDirNames() const {
    std::vector<std::string> names;
    for (const auto& entry :
         std::filesystem::directory_iterator{stateDirPath_.asString()}) {
      names.push_back(entry.path().filename().string());
    }
    std::sort(names.begin(), names.end());
    return names;
  }

  /** Every name in the state directory a sentinel prefix scan would match. */
  [[maybe_unused]]
  std::vector<std::string> sentinelNames() const {
    auto names = stateDirNames();
    std::erase_if(names, [this](const std::string& name) {
      return !std::string_view{name}.starts_with(
          stateDir_.getRestartSentinelNamePrefix());
    });
    return names;
  }
#endif

  /** Creates an empty file in the state directory and returns its path. */
  [[maybe_unused]]
  AbsolutePath makeStateDirFile(std::string_view name) const {
    const auto path = stateDirPath_ + PathComponent{std::string{name}};
    EXPECT_TRUE(folly::writeFile(std::string{}, path.c_str())) << name;
    return path;
  }

  /** Creates a directory in the state directory and returns its path. */
  [[maybe_unused]]
  AbsolutePath makeStateDirSubdir(std::string_view name) const {
    const auto path = stateDirPath_ + PathComponent{std::string{name}};
    std::error_code ec;
    std::filesystem::create_directory(path.c_str(), ec);
    EXPECT_FALSE(ec) << name << ": " << ec.message();
    return path;
  }

  folly::test::TemporaryDirectory tmpDir_;
  AbsolutePath stateDirPath_;
  EdenStateDir stateDir_;
  AbsolutePath daemonArgsPath_;
  std::shared_ptr<EdenConfig> edenConfig_;
  RecordingPrivHelper privHelper_;
};

TEST_F(RestartArmerTest, aFreshArmerIsDisarmed) {
  EXPECT_FALSE(makeArmer().armed());
}

TEST_F(RestartArmerTest, disarmingWithoutArmingLeavesOtherGenerationsAlone) {
  const auto foreign =
      stateDirPath_ + ".edenfs_restart_armed.1.0000000000000002"_pc;
  ASSERT_TRUE(folly::writeFile(std::string{"stale"}, foreign.c_str()));

  auto armer = makeArmer();
  armer.removeSentinel();

  EXPECT_TRUE(exists(foreign));
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

TEST_F(RestartArmerTest, armingCreatesAnEmptySentinelForThisGeneration) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  makeArmer().arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  const auto sentinel = armedSentinelPath();
  EXPECT_EQ(stateDirPath_.asString(), sentinel.dirname().asString());

  const auto leaf = std::string{sentinel.basename().view()};
  const auto expectedPrefix =
      ".edenfs_restart_armed." + std::to_string(::getpid()) + ".";
  EXPECT_TRUE(leaf.starts_with(expectedPrefix)) << leaf;
  EXPECT_EQ(expectedPrefix.size() + 16, leaf.size());

  struct stat st{};
  ASSERT_EQ(0, ::stat(sentinel.c_str(), &st));
  EXPECT_EQ(0600, st.st_mode & 07777);
  EXPECT_EQ(0, st.st_size);

  EXPECT_EQ(std::vector<std::string>{leaf}, sentinelNames());
}

TEST_F(RestartArmerTest, rearmingReplacesThePreviousSentinel) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  auto armer = makeArmer();
  armer.arm();
  const auto first = armedSentinelPath();
  armer.arm();
  const auto second = armedSentinelPath();

  EXPECT_NE(first.asString(), second.asString());
  EXPECT_FALSE(exists(first));
  EXPECT_TRUE(exists(second));
  EXPECT_EQ(1, sentinelNames().size());
}

TEST_F(RestartArmerTest, armingReapsTheSentinelsOfOtherGenerations) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  makeStateDirFile(".edenfs_restart_armed.1.0000000000000001");
  makeStateDirFile(".edenfs_restart_armed.999999.00000000000000ff");

  makeArmer().arm();

  const auto own = std::string{armedSentinelPath().basename().view()};
  EXPECT_EQ(std::vector<std::string>{own}, sentinelNames());
}

TEST_F(RestartArmerTest, armingReapsBeforeTheRequestIsAnswered) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  const auto foreign =
      makeStateDirFile(".edenfs_restart_armed.1.0000000000000001");
  privHelper_.pendingRequest.emplace();

  auto armer = makeArmer();
  armer.arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  ASSERT_FALSE(armer.armed());
  EXPECT_FALSE(exists(foreign));

  privHelper_.pendingRequest->setValue();
  EXPECT_TRUE(armer.armed());
}

TEST_F(RestartArmerTest, anArmThatNeverGetsASentinelReapsNothing) {
  const auto foreign =
      makeStateDirFile(".edenfs_restart_armed.1.0000000000000001");

  makeArmer().arm();

  ASSERT_TRUE(privHelper_.restartArgs.empty());
  EXPECT_TRUE(exists(foreign));
}

TEST_F(RestartArmerTest, aSentinelThatCannotBeUnlinkedDoesNotFailTheArm) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  // Directories carry sentinel names no daemon would give them, but unlinkat
  // refuses them, which is the per-entry failure under test.
  const auto first =
      makeStateDirSubdir(".edenfs_restart_armed.1.0000000000000001");
  const auto second =
      makeStateDirSubdir(".edenfs_restart_armed.2.0000000000000002");

  auto armer = makeArmer();
  armer.arm();

  EXPECT_TRUE(armer.armed());
  EXPECT_TRUE(exists(armedSentinelPath()));
  EXPECT_TRUE(exists(first));
  EXPECT_TRUE(exists(second));
}

TEST_F(RestartArmerTest, aNameThatOnlySharesThePrefixIsNotReaped) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  const auto generationless = makeStateDirFile(".edenfs_restart_armed");
  std::vector<std::string> kept{
      ".edenfs_restart_armed.Ab3XyZ",
      ".edenfs_restart_armed.7.000000000000000",
      ".edenfs_restart_armed.7.0000000000000001x",
      ".edenfs_restart_armed.7.0123456789ABCDEF",
      ".edenfs_restart_armed..0123456789abcdef",
      ".edenfs_restart_armed.pid.0123456789abcdef",
  };
  for (const auto& name : kept) {
    makeStateDirFile(name);
  }
  makeStateDirFile(".edenfs_restart_armed.7.0123456789abcdef");

  makeArmer().arm();

  kept.push_back(std::string{armedSentinelPath().basename().view()});
  std::sort(kept.begin(), kept.end());
  EXPECT_EQ(kept, sentinelNames());
  EXPECT_TRUE(exists(generationless));
}

TEST_F(RestartArmerTest, unrelatedFilesInTheStateDirAreLeftAlone) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  for (const auto* name :
       {"edenfs_restart_armed.1.0000000000000001", "heartbeat_1", "lock"}) {
    makeStateDirFile(name);
  }

  makeArmer().arm();

  std::vector<std::string> expected{
      ".edenfs_start_args",
      "edenfs_restart_armed.1.0000000000000001",
      "heartbeat_1",
      "lock",
      std::string{armedSentinelPath().basename().view()},
  };
  std::sort(expected.begin(), expected.end());
  EXPECT_EQ(expected, stateDirNames());
}

TEST_F(RestartArmerTest, disarmingRemovesTheSentinelThatWasCreated) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  auto armer = makeArmer();
  armer.arm();
  const auto sentinel = armedSentinelPath();
  ASSERT_TRUE(exists(sentinel));

  armer.removeSentinel();
  ASSERT_FALSE(exists(sentinel));

  armer.removeSentinel();
  EXPECT_FALSE(exists(sentinel));
}

TEST_F(RestartArmerTest, armingSendsTheRequestAndMarksItselfArmed) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));

  auto armer = makeArmer();
  armer.arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  const auto& args = privHelper_.restartArgs.front();
  EXPECT_TRUE(args.enabled);
  EXPECT_TRUE(exists(armedSentinelPath()));
  EXPECT_NE(0, args.sentinelNonce);
  EXPECT_TRUE(armer.armed());
}

TEST_F(RestartArmerTest, aRejectedRequestRemovesTheSentinelAgain) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  privHelper_.setRestartArgsError =
      folly::make_exception_wrapper<std::runtime_error>("unknown request");

  auto armer = makeArmer();
  armer.arm();

  ASSERT_EQ(1, privHelper_.restartArgs.size());
  EXPECT_FALSE(armer.armed());
  EXPECT_FALSE(exists(armedSentinelPath()));
}

TEST_F(RestartArmerTest, aMissingDaemonArgsFileDoesNotArm) {
  auto armer = makeArmer();
  armer.arm();

  EXPECT_FALSE(armer.armed());
  EXPECT_TRUE(sentinelNames().empty());
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
  EXPECT_TRUE(sentinelNames().empty());
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
    ASSERT_TRUE(exists(armedSentinelPath()));
  }

  privHelper_.pendingRequest->setException(
      std::runtime_error("privhelper went away"));

  EXPECT_FALSE(exists(armedSentinelPath()));
}

TEST_F(RestartArmerTest, aLateRejectionRemovesOnlyTheSentinelOfItsOwnArm) {
  ASSERT_TRUE(folly::writeFile(kDaemonArgs.str(), daemonArgsPath_.c_str()));
  privHelper_.pendingRequest.emplace();

  auto armer = makeArmer();
  armer.arm();
  const auto first = armedSentinelPath();

  // The re-arm a failed takeover would do: same pid, and answered at once.
  auto firstRequest = std::move(*privHelper_.pendingRequest);
  privHelper_.pendingRequest.reset();
  armer.arm();
  const auto second = armedSentinelPath();
  ASSERT_NE(first.asString(), second.asString());

  firstRequest.setException(std::runtime_error("privhelper went away"));

  EXPECT_FALSE(exists(first));
  EXPECT_TRUE(exists(second));
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

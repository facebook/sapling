/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/takeover/TakeoverServer.h"

using namespace facebook::eden;
using folly::EventBase;
using folly::Future;
using folly::makeFuture;
using folly::Promise;
using folly::test::TemporaryDirectory;
using std::string;
using ::testing::ElementsAre;
using ::testing::ElementsAreArray;
using namespace std::chrono_literals;

namespace {
/**
 * A TakeoverHandler that returns the TakeoverData object passed to its
 * constructor.
 */
class TestHandler : public TakeoverHandler {
 public:
  explicit TestHandler(TakeoverData&& data) : data_{std::move(data)} {}

  Future<TakeoverData> startTakeoverShutdown() override {
    return makeFuture(std::move(data_));
  }

 private:
  TakeoverData data_;
};

/**
 * A TakeoverHandler that throws an exception.
 */
class ErrorHandler : public TakeoverHandler {
 public:
  Future<TakeoverData> startTakeoverShutdown() override {
    return makeFuture<TakeoverData>(
        std::logic_error("purposely failing for testing"));
  }
};

/**
 * Run takeoverMounts() in a separate thread, and return a Future that will
 * complete in the specified EventBase once takeoverMounts() finishes.
 */
Future<TakeoverData> takeoverViaEventBase(
    EventBase* evb,
    AbsolutePathPiece socketPath,
    const std::set<int32_t>& supportedVersions) {
  Promise<TakeoverData> promise;
  auto future = promise.getFuture();
  std::thread thread([path = AbsolutePath{socketPath},
                      supportedVersions,
                      promise = std::move(promise)]() mutable {
    promise.setWith([&] { return takeoverMounts(path, supportedVersions); });
  });

  return future.via(evb).ensure(
      [t = std::move(thread)]() mutable { t.join(); });
}

/**
 * A helper class to terminate the EventBase loop if the test runs
 * for longer than we expect.  This ensures the test won't run forever if
 * something goes wrong.
 *
 * This is used by the loopWithTimeout() helper function below.
 */
class TestTimeout : public folly::AsyncTimeout {
 public:
  explicit TestTimeout(EventBase* evb)
      : folly::AsyncTimeout{evb}, eventBase_{evb} {}

  void timeoutExpired() noexcept override {
    ADD_FAILURE() << "test timeout expired";
    eventBase_->terminateLoopSoon();
  }

 private:
  EventBase* eventBase_{nullptr};
};

void loopWithTimeout(EventBase* evb, std::chrono::milliseconds timeout = 300s) {
  TestTimeout timeoutObject{evb};
  timeoutObject.scheduleTimeout(timeout);
  evb->loop();
}

/**
 * Create a TakeoverServer using the specified TakeoverHandler, then call
 * takeoverMounts() to receive the TakeoverData from it.  Returns a
 * Try<TakeoverData> with the result.
 */
folly::Try<TakeoverData> runTakeover(
    const TemporaryDirectory& tmpDir,
    TakeoverHandler* handler,
    const std::set<int32_t>& supportedVersions = kSupportedTakeoverVersions) {
  // Ignore SIGPIPE so that sendmsg() will fail with an error code instead
  // of terminating the program if the remote side has closed the connection.
  signal(SIGPIPE, SIG_IGN);

  AbsolutePath socketPath =
      AbsolutePathPiece{tmpDir.path().string()} + "takeover"_pc;
  EventBase evb;

  TakeoverServer server(&evb, socketPath, handler);

  auto future =
      takeoverViaEventBase(&evb, socketPath, supportedVersions).ensure([&] {
        evb.terminateLoopSoon();
      });
  loopWithTimeout(&evb);
  if (!future.isReady()) {
    // This should generally only happen if we timed out.
    throw std::runtime_error("future is not ready");
  }
  return std::move(future.getTry());
}

void checkExpectedFile(int fd, AbsolutePathPiece path) {
  struct stat fdStat;
  struct stat pathStat;
  auto rc = fstat(fd, &fdStat);
  folly::checkUnixError(rc, "fstat failed");
  rc = stat(path.stringPiece().str().c_str(), &pathStat);
  folly::checkUnixError(rc, "stat failed");

  EXPECT_EQ(fdStat.st_dev, pathStat.st_dev);
  EXPECT_EQ(fdStat.st_ino, pathStat.st_ino);
}
} // namespace

TEST(Takeover, simple) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  AbsolutePathPiece tmpDirPath{tmpDir.path().string()};

  // Build the TakeoverData object to send
  TakeoverData serverData;

  auto lockFilePath = tmpDirPath + "lock"_pc;
  serverData.lockFile =
      folly::File{lockFilePath.stringPiece(), O_RDWR | O_CREAT};

  auto thriftSocketPath = tmpDirPath + "thrift"_pc;
  serverData.thriftSocket =
      folly::File{thriftSocketPath.stringPiece(), O_RDWR | O_CREAT};

  auto mount1Path = tmpDirPath + "mount1"_pc;
  auto client1Path = tmpDirPath + "client1"_pc;
  auto mount1FusePath = tmpDirPath + "fuse1"_pc;
  serverData.mountPoints.emplace_back(
      mount1Path,
      client1Path,
      std::vector<AbsolutePath>{},
      folly::File{mount1FusePath.stringPiece(), O_RDWR | O_CREAT},
      fuse_init_out{},
      SerializedInodeMap{});

  auto mount2Path = tmpDirPath + "mount2"_pc;
  auto client2Path = tmpDirPath + "client2"_pc;
  auto mount2FusePath = tmpDirPath + "fuse2"_pc;
  std::vector<AbsolutePath> mount2BindMounts = {
      mount2Path + "test/test2"_relpath,
      AbsolutePath{"/foo/bar"},
      mount2Path + "a/b/c/d/e/f"_relpath,
  };
  serverData.mountPoints.emplace_back(
      mount2Path,
      client2Path,
      mount2BindMounts,
      folly::File{mount2FusePath.stringPiece(), O_RDWR | O_CREAT},
      fuse_init_out{},
      SerializedInodeMap{});

  // Perform the takeover
  auto serverSendFuture = serverData.takeoverComplete.getFuture();
  TestHandler handler{std::move(serverData)};
  auto result = runTakeover(tmpDir, &handler);
  ASSERT_TRUE(serverSendFuture.hasValue());
  EXPECT_TRUE(result.hasValue());
  const auto& clientData = result.value();

  // Make sure the received lock file refers to the expected file.
  checkExpectedFile(clientData.lockFile.fd(), lockFilePath);
  // And the thrift socket FD
  checkExpectedFile(clientData.thriftSocket.fd(), thriftSocketPath);

  // Make sure the received mount information is correct
  ASSERT_EQ(2, clientData.mountPoints.size());
  EXPECT_EQ(mount1Path, clientData.mountPoints.at(0).mountPath);
  EXPECT_EQ(client1Path, clientData.mountPoints.at(0).stateDirectory);
  EXPECT_THAT(clientData.mountPoints.at(0).bindMounts, ElementsAre());
  checkExpectedFile(clientData.mountPoints.at(0).fuseFD.fd(), mount1FusePath);

  EXPECT_EQ(mount2Path, clientData.mountPoints.at(1).mountPath);
  EXPECT_EQ(client2Path, clientData.mountPoints.at(1).stateDirectory);
  EXPECT_THAT(
      clientData.mountPoints.at(1).bindMounts,
      ElementsAreArray(mount2BindMounts));
  checkExpectedFile(clientData.mountPoints.at(1).fuseFD.fd(), mount2FusePath);
}

TEST(Takeover, noMounts) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  AbsolutePathPiece tmpDirPath{tmpDir.path().string()};

  // Build the TakeoverData object with no mount points
  TakeoverData serverData;
  auto lockFilePath = tmpDirPath + "lock"_pc;
  serverData.lockFile =
      folly::File{lockFilePath.stringPiece(), O_RDWR | O_CREAT};
  auto thriftSocketPath = tmpDirPath + "thrift"_pc;
  serverData.thriftSocket =
      folly::File{thriftSocketPath.stringPiece(), O_RDWR | O_CREAT};

  // Perform the takeover
  auto serverSendFuture = serverData.takeoverComplete.getFuture();
  TestHandler handler{std::move(serverData)};
  auto result = runTakeover(tmpDir, &handler);
  ASSERT_TRUE(serverSendFuture.hasValue());
  EXPECT_TRUE(result.hasValue());
  const auto& clientData = result.value();

  // Make sure the received lock file and thrift socket FD refer to the
  // expected files.
  checkExpectedFile(clientData.lockFile.fd(), lockFilePath);
  checkExpectedFile(clientData.thriftSocket.fd(), thriftSocketPath);

  // Make sure the received mount information is empty
  EXPECT_EQ(0, clientData.mountPoints.size());
}

TEST(Takeover, manyMounts) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  AbsolutePathPiece tmpDirPath{tmpDir.path().string()};

  // Build the TakeoverData object
  TakeoverData serverData;
  auto lockFilePath = tmpDirPath + "lock"_pc;
  serverData.lockFile =
      folly::File{lockFilePath.stringPiece(), O_RDWR | O_CREAT};
  auto thriftSocketPath = tmpDirPath + "thrift"_pc;
  serverData.thriftSocket =
      folly::File{thriftSocketPath.stringPiece(), O_RDWR | O_CREAT};

  // Build info for 10,000 mounts
  // This exercises the code where we send more FDs than ControlMsg::kMaxFDs.
  //
  // Note that for this test to succeed your "ulimit -n" settings must be at
  // least twice this number.  We will end up with 2 FDs for each mount, since
  // we open one on the "server" side, and then the client receives a copy of
  // each FD.
  constexpr size_t numMounts = 10000;
  for (size_t n = 0; n < numMounts; ++n) {
    auto mountPath =
        tmpDirPath + RelativePathPiece{folly::to<string>("mounts/foo/test", n)};
    auto stateDirectory =
        tmpDirPath + RelativePathPiece{folly::to<string>("client", n)};
    // Define 0 to 9 bind mounts
    std::vector<AbsolutePath> bindMounts;
    for (size_t b = 0; b < n % 10; ++b) {
      bindMounts.emplace_back(
          mountPath + RelativePathPiece{folly::to<string>("bind_mount", b)});
    }
    auto fusePath =
        tmpDirPath + PathComponentPiece{folly::to<string>("fuse", n)};
    serverData.mountPoints.emplace_back(
        mountPath,
        stateDirectory,
        bindMounts,
        folly::File{fusePath.stringPiece(), O_RDWR | O_CREAT},
        fuse_init_out{},
        SerializedInodeMap{});
  }

  // Perform the takeover
  auto serverSendFuture = serverData.takeoverComplete.getFuture();
  TestHandler handler{std::move(serverData)};
  auto result = runTakeover(tmpDir, &handler);
  ASSERT_TRUE(serverSendFuture.hasValue());
  EXPECT_TRUE(result.hasValue());
  const auto& clientData = result.value();

  // Make sure the received lock file and thrift socket FDs are correct
  checkExpectedFile(clientData.lockFile.fd(), lockFilePath);
  checkExpectedFile(clientData.thriftSocket.fd(), thriftSocketPath);

  // Make sure the received mount information is correct
  ASSERT_EQ(numMounts, clientData.mountPoints.size());
  for (size_t n = 0; n < numMounts; ++n) {
    const auto& mountInfo = clientData.mountPoints[n];
    auto expectedMountPath =
        tmpDirPath + RelativePathPiece{folly::to<string>("mounts/foo/test", n)};
    EXPECT_EQ(expectedMountPath, mountInfo.mountPath);

    auto expectedClientPath =
        tmpDirPath + RelativePathPiece{folly::to<string>("client", n)};
    EXPECT_EQ(expectedClientPath, mountInfo.stateDirectory);

    std::vector<AbsolutePath> expectedBindMounts;
    for (size_t b = 0; b < n % 10; ++b) {
      expectedBindMounts.emplace_back(
          expectedMountPath +
          RelativePathPiece{folly::to<string>("bind_mount", b)});
    }
    EXPECT_THAT(mountInfo.bindMounts, ElementsAreArray(expectedBindMounts));

    auto expectedFusePath =
        tmpDirPath + PathComponentPiece{folly::to<string>("fuse", n)};
    checkExpectedFile(mountInfo.fuseFD.fd(), expectedFusePath);
  }
}

TEST(Takeover, error) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  ErrorHandler handler;
  auto result = runTakeover(tmpDir, &handler);
  EXPECT_THROW_RE(
      result.value(),
      std::runtime_error,
      "logic_error: purposely failing for testing");
}

TEST(Takeover, computeCompatibleVersion) {
  const std::set<int32_t> noVersions;
  const std::set<int32_t> oneVersion{1};
  const std::set<int32_t> newVersion{1, 2};
  const std::set<int32_t> newerVersion{2, 3};
  const std::set<int32_t> newestVersion{3, 4};
  const std::set<int32_t> laundryList{1, 2, 3, 4};

  // Check that computeCompatibleVersion is doing the right things.
  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(noVersions, oneVersion),
      std::nullopt);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(oneVersion, oneVersion).value(),
      1);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(oneVersion, newVersion).value(),
      1);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(newVersion, newerVersion).value(),
      2);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(newerVersion, newestVersion)
          .value(),
      3);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(newVersion, newestVersion),
      std::nullopt);

  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(newestVersion, laundryList)
          .value(),
      4);

  // Try it with the parameters flipped; we should still have the
  // same output.
  EXPECT_EQ(
      TakeoverData::computeCompatibleVersion(laundryList, newestVersion)
          .value(),
      4);
}

TEST(Takeover, errorVersionMismatch) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  ErrorHandler handler;
  auto result = runTakeover(
      tmpDir,
      &handler,
      std::set<int32_t>{TakeoverData::kTakeoverProtocolVersionNeverSupported});
  EXPECT_THROW_RE(
      result.value(),
      std::runtime_error,
      "The client and the server do not share a common "
      "takeover protocol implementation.");
}

TEST(Takeover, oneToTwo) {
  TemporaryDirectory tmpDir("eden_takeover_test");
  AbsolutePathPiece tmpDirPath{tmpDir.path().string()};

  // Build the TakeoverData object with no mount points
  TakeoverData serverData;
  auto lockFilePath = tmpDirPath + "lock"_pc;
  serverData.lockFile =
      folly::File{lockFilePath.stringPiece(), O_RDWR | O_CREAT};
  auto thriftSocketPath = tmpDirPath + "thrift"_pc;
  serverData.thriftSocket =
      folly::File{thriftSocketPath.stringPiece(), O_RDWR | O_CREAT};

  // Perform the takeover, explicitly using the older version
  // of the takeover protocol
  auto serverSendFuture = serverData.takeoverComplete.getFuture();
  TestHandler handler{std::move(serverData)};
  auto result = runTakeover(
      tmpDir,
      &handler,
      std::set<int32_t>{TakeoverData::kTakeoverProtocolVersionOne});
  ASSERT_TRUE(serverSendFuture.hasValue());
  EXPECT_TRUE(result.hasValue());
  const auto& clientData = result.value();

  // Make sure the received lock file and thrift socket FD refer to the
  // expected files.
  checkExpectedFile(clientData.lockFile.fd(), lockFilePath);
  checkExpectedFile(clientData.thriftSocket.fd(), thriftSocketPath);

  // Make sure the received mount information is empty
  EXPECT_EQ(0, clientData.mountPoints.size());
}

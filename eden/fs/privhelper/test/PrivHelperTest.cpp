/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Portability.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventBaseThread.h>
#include <folly/json/json.h>
#include <folly/synchronization/Baton.h>
#include <folly/synchronization/SaturatingSemaphore.h>
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <sys/wait.h>
#include <atomic>
#include <chrono>
#include <optional>
#include <thread>
#include <unordered_map>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/UserInfo.h"
#include "eden/common/utils/test/ScopedEnvVar.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/privhelper/PrivHelperConn.h"
#include "eden/fs/privhelper/PrivHelperImpl.h"
#include "eden/fs/privhelper/test/PrivHelperTestServer.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/XplatKeys.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using facebook::eden::UserInfo;
using folly::checkUnixError;
using folly::EventBase;
using folly::EventBaseThread;
using folly::File;
using folly::Future;
using folly::Promise;
using folly::StringPiece;
using folly::Unit;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;
using std::string;
using testing::UnorderedElementsAre;

TEST(TccDisclaimKillswitch, presentWhenFileExists) {
  TemporaryFile killswitch;
  EXPECT_TRUE(tccDisclaimKillswitchPresent(killswitch.path().c_str()));
}

TEST(TccDisclaimKillswitch, absentWhenFileDoesNotExist) {
  TemporaryDirectory dir;
  auto missing = (dir.path() / "disable-tcc-disclaim").string();
  EXPECT_FALSE(tccDisclaimKillswitchPresent(missing.c_str()));
}

/**
 * A PrivHelperServer implementation intended to be used in a separate thread in
 * the same process.
 *
 * This is different than PrivHelperTestServer which is intended to be used in a
 * separate forked process.
 */
class PrivHelperThreadedTestServer : public PrivHelperServer {
 public:
  Promise<File> setFuseMountResult(StringPiece path) {
    Promise<File> promise;
    {
      auto data = data_.wlock();
      auto results =
          data->fuseMountResults.emplace(path.str(), std::list<Future<File>>{});
      results.first->second.emplace_back(promise.getFuture());
    }
    return promise;
  }

  Promise<File> setFuseRetryMountResult(StringPiece path) {
    Promise<File> promise;
    data_.wlock()
        ->fuseMountResults.find(path.str())
        ->second.emplace_back(promise.getFuture());
    return promise;
  }

  Promise<Unit> setFuseUnmountResult(StringPiece path) {
    Promise<Unit> promise;
    {
      auto data = data_.wlock();
      auto results = data->fuseUnmountResults.emplace(
          path.str(), std::list<Future<Unit>>{});
      results.first->second.emplace_back(promise.getFuture());
    }
    return promise;
  }

  Promise<Unit> setBindMountResult(StringPiece path) {
    Promise<Unit> promise;
    {
      auto data = data_.wlock();
      auto results =
          data->bindMountResults.emplace(path.str(), std::list<Future<Unit>>{});
      results.first->second.emplace_back(promise.getFuture());
    }
    return promise;
  }

  Promise<Unit> setBindUnmountResult(StringPiece path) {
    Promise<Unit> promise;
    {
      auto data = data_.wlock();
      auto results = data->bindUnmountResults.emplace(
          path.str(), std::list<Future<Unit>>{});
      results.first->second.emplace_back(promise.getFuture());
    }
    return promise;
  }

  std::vector<string> getUnusedFuseUnmountResults() {
    return getUnusedResults(data_.rlock()->fuseUnmountResults);
  }

  std::vector<string> getUnusedBindUnmountResults() {
    return getUnusedResults(data_.rlock()->bindUnmountResults);
  }

  std::vector<File> getLogFileRequests() {
    auto data = data_.wlock();
    return std::move(data->logFiles);
  }

  std::vector<std::string> getRequestedVfsTypes() {
    auto data = data_.wlock();
    return std::move(data->requestedVfsTypes);
  }

 private:
  struct Data {
    std::unordered_map<string, std::list<Future<File>>> fuseMountResults;
    std::vector<std::string> requestedVfsTypes;
    std::unordered_map<string, std::list<Future<Unit>>> fuseUnmountResults;
    std::unordered_map<string, std::list<Future<Unit>>> bindMountResults;
    std::unordered_map<string, std::list<Future<Unit>>> bindUnmountResults;
    std::vector<File> logFiles;
  };

  template <typename T>
  folly::Future<T> getResultFuture(
      std::unordered_map<string, std::list<Future<T>>>& map,
      StringPiece path) {
    auto iter = map.find(path.str());
    if (iter == map.end()) {
      throw std::runtime_error(
          folly::to<string>("no result available for ", path));
    }
    auto future = std::move(iter->second.front());
    iter->second.pop_front();
    if (iter->second.empty()) {
      map.erase(iter);
    }
    return future;
  }

  template <typename T>
  std::vector<string> getUnusedResults(
      const std::unordered_map<std::string, std::list<Future<T>>>& map) {
    std::vector<string> results;
    for (const auto& entry : map) {
      results.push_back(entry.first);
    }
    return results;
  }

  folly::File fuseMount(
      const char* mountPath,
      bool /*readOnly*/,
      const char* vfsType) override {
    Future<folly::File> future = Future<folly::File>::makeEmpty();
    {
      auto data = data_.wlock();
      data->requestedVfsTypes.emplace_back(vfsType);
      future = getResultFuture(data->fuseMountResults, mountPath);
    }
    return std::move(future).get(1s);
  }

  void unmount(const char* mountPath, UnmountOptions /* options */) override {
    auto future = getResultFuture(data_.wlock()->fuseUnmountResults, mountPath);
    std::move(future).get(1s);

    // fuseUnmount has the side effect of implicitly unmounting all contained
    // bind mounts, so let's make that appear to be the case here.
    // This loop is the C++20 suggested impl of erase_if, but inlined here for
    // environments that are not yet C++20.
    auto data = data_.wlock();
    auto mountPrefix = folly::to<std::string>(mountPath, "/");
    for (auto iter = data->bindUnmountResults.begin(),
              last = data->bindUnmountResults.end();
         iter != last;) {
      if (folly::StringPiece(iter->first).startsWith(mountPrefix)) {
        iter = data->bindUnmountResults.erase(iter);
      } else {
        ++iter;
      }
    }
  }

  void insecureBindMount(const char* /* clientPath */, const char* mountPath)
      override {
    auto future = getResultFuture(data_.wlock()->bindMountResults, mountPath);
    std::move(future).get(1s);
  }

  void bindMount(
      const char* /* clientPath */,
      const char* mountPath,
      folly::StringPiece mountRoot) override {
#ifndef __APPLE__
    static_cast<void>(openBindMountTarget(mountRoot, mountPath));
#else
    (void)mountRoot;
#endif
    auto future = getResultFuture(data_.wlock()->bindMountResults, mountPath);
    std::move(future).get(1s);
  }

  void insecureBindUnmount(const char* mountPath) override {
    auto future = getResultFuture(data_.wlock()->bindUnmountResults, mountPath);
    std::move(future).get(1s);
  }

  void bindUnmount(const char* mountPath, folly::StringPiece mountRoot)
      override {
#ifndef __APPLE__
    static_cast<void>(openBindMountTarget(mountRoot, mountPath));
#else
    (void)mountRoot;
#endif
    auto future = getResultFuture(data_.wlock()->bindUnmountResults, mountPath);
    std::move(future).get(1s);
  }

  bool useModernMountApi() const override {
    return false;
  }

  void setLogFile(folly::File logFile) override {
    auto data = data_.wlock();
    data->logFiles.push_back(std::move(logFile));
  }

  folly::Synchronized<Data> data_;
};

class PrivHelperFdUnmountTestServer : public PrivHelperServer {
 public:
  bool insecureBindUnmountCalled() const {
    return insecureBindUnmountCalled_.load();
  }

 private:
  void unmount(const char* /* mountPath */, UnmountOptions /* options */)
      override {}

  void insecureBindUnmount(const char* /* mountPath */) override {
    insecureBindUnmountCalled_.store(true);
    throw std::runtime_error("unexpected path-based bind unmount fallback");
  }

#ifndef __APPLE__
  int umountBindMountByFd(const char* /* procFdPath */) override {
    errno = EINVAL;
    return -1;
  }
#endif

  std::atomic<bool> insecureBindUnmountCalled_{false};
};

namespace {

UnixSocket::Message makeLegacyMacFuseConfigRequest(
    uint32_t xid,
    uint32_t requestId,
    uint64_t value) {
  constexpr uint32_t kProtocolVersion = 1;
  constexpr uint32_t kMetadataLength = 8;
  constexpr size_t kRequestSize = 4 * sizeof(uint32_t) + sizeof(uint64_t);

  UnixSocket::Message request;
  request.data = folly::IOBuf(folly::IOBuf::CREATE, kRequestSize);
  folly::io::Appender appender(&request.data, kRequestSize);
  appender.write<uint32_t>(kProtocolVersion);
  appender.write<uint32_t>(kMetadataLength);
  appender.write<uint32_t>(xid);
  appender.write<uint32_t>(requestId);
  appender.write<uint64_t>(value);
  return request;
}

static_assert(PrivHelperConn::REQ_SET_DAEMON_TIMEOUT == 9);
static_assert(PrivHelperConn::REQ_SET_USE_EDENFS == 10);

// An arbitrary fixed point in time, so that the restart window can be aged
// without sleeping.
constexpr uint64_t kFakeNow = 1'700'000'000ull;

// Distinct bytes above 2^32, so a truncated width or a swapped field fails.
constexpr uint64_t kSentinelNonce = 0x0123456789abcdefull;

EdenFsRestartArgs makeRestartArgs(std::string sentinelPath) {
  EdenFsRestartArgs args;
  args.enabled = true;
  args.sentinelPath = std::move(sentinelPath);
  args.sentinelNonce = kSentinelNonce;
  args.restartCount = 1;
  args.firstRestartEpochSec = kFakeNow;
  args.maxRestarts = 3;
  args.windowSeconds = 600;
  return args;
}

#ifdef __APPLE__
const std::vector<std::string> kSentinelArgv{
    "/usr/local/libexec/eden/edenfs",
    "--edenfs"};

/** The relaunch command as EdenServer::armPrivHelperRestart() writes it. */
std::string makeSentinelContents(uint64_t nonce = kSentinelNonce) {
  folly::dynamic argv = folly::dynamic::array;
  for (const auto& arg : kSentinelArgv) {
    argv.push_back(arg);
  }
  return folly::toJson(
      folly::dynamic::object("argv", argv)(
          "env",
          folly::dynamic::object("PATH", "/usr/bin")("HOME", "/home/test"))(
          "nonce", static_cast<int64_t>(nonce)));
}

/** As a daemon too old to stamp a generation writes it. */
std::string makeSentinelContentsWithoutNonce() {
  return folly::toJson(
      folly::dynamic::object("argv", folly::dynamic::array("/bin/edenfs")));
}
#endif // __APPLE__

EdenFsRestartArgs roundTrip(const EdenFsRestartArgs& args) {
  auto msg = PrivHelperConn::serializeSetRestartArgsRequest(/*xid=*/42, args);
  folly::io::Cursor cursor{&msg.data};
  PrivHelperConn::parsePacket(cursor);

  EdenFsRestartArgs parsed;
  PrivHelperConn::parseSetRestartArgsRequest(cursor, parsed);
  return parsed;
}

} // namespace

TEST(PrivHelperConnRestartArgs, roundTripPreservesAwkwardValues) {
  auto expected =
      makeRestartArgs("/var/eden dir/.edenfs_restart_armed \xc3\xa9");
  // Above 2^32, to catch a truncated width on the wire.
  expected.firstRestartEpochSec = uint64_t{1} << 33;

  EXPECT_EQ(expected, roundTrip(expected));
}

TEST(PrivHelperRestartCounterEnv, absentIsZero) {
  ScopedEnvVar var{kEdenFsRestartCountEnv};
  var.unset();
  EXPECT_EQ(0, readEdenFsRestartCounterEnv(kEdenFsRestartCountEnv));
}

TEST(PrivHelperRestartCounterEnv, emptyIsZero) {
  ScopedEnvVar var{kEdenFsRestartCountEnv};
  var.set("");
  EXPECT_EQ(0, readEdenFsRestartCounterEnv(kEdenFsRestartCountEnv));
}

TEST(PrivHelperRestartCounterEnv, malformedIsZero) {
  ScopedEnvVar var{kEdenFsRestartCountEnv};
  var.set("three");
  EXPECT_EQ(0, readEdenFsRestartCounterEnv(kEdenFsRestartCountEnv));
}

TEST(PrivHelperRestartCounterEnv, readsAValueAbove32Bits) {
  // The epoch variable outgrows uint32 in 2106, so the reader is 64-bit.
  ScopedEnvVar var{kEdenFsFirstRestartAtEnv};
  var.set("4294967296");
  EXPECT_EQ(
      uint64_t{1} << 32, readEdenFsRestartCounterEnv(kEdenFsFirstRestartAtEnv));
}

TEST(PrivHelperConnRestartArgs, notifyCleanShutdownRoundTrip) {
  constexpr folly::StringPiece kReason{"graceful restart"};
  auto msg = PrivHelperConn::serializeNotifyCleanShutdownRequest(
      /*xid=*/7, kReason);
  folly::io::Cursor cursor{&msg.data};
  PrivHelperConn::parsePacket(cursor);

  std::string reason;
  PrivHelperConn::parseNotifyCleanShutdownRequest(cursor, reason);
  EXPECT_EQ(kReason, reason);
}

class RawPrivHelperClient : private UnixSocket::ReceiveCallback {
 public:
  explicit RawPrivHelperClient(File conn) {
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
        [this, conn = std::move(conn)]() mutable {
          conn_ = UnixSocket::makeUnique(
              clientIoThread_.getEventBase(), std::move(conn));
          conn_->setReceiveCallback(this);
        });
  }

  ~RawPrivHelperClient() override {
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait([this] {
      if (conn_) {
        conn_->clearReceiveCallback();
        conn_->closeNow();
        conn_.reset();
      }
    });
  }

  /** Sends without expecting a reply, as a one-way request does. */
  void send(UnixSocket::Message request) {
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
        [this, request = std::move(request)]() mutable {
          conn_->send(std::move(request));
        });
  }

  UnixSocket::Message sendAndRecv(UnixSocket::Message request) {
    Promise<UnixSocket::Message> promise;
    auto future = promise.getFuture();
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
        [this,
         request = std::move(request),
         promise = std::move(promise)]() mutable {
          responsePromise_ = std::move(promise);
          conn_->send(std::move(request));
        });
    return std::move(future).get(1s);
  }

 private:
  void messageReceived(UnixSocket::Message&& message) noexcept override {
    if (responsePromise_) {
      std::move(*responsePromise_).setValue(std::move(message));
      responsePromise_.reset();
    }
  }

  void eofReceived() noexcept override {
    setResponseException(
        folly::make_exception_wrapper<std::runtime_error>("privhelper exited"));
  }

  void socketClosed() noexcept override {
    setResponseException(
        folly::make_exception_wrapper<std::runtime_error>(
            "privhelper client socket closed"));
  }

  void receiveError(const folly::exception_wrapper& ew) noexcept override {
    setResponseException(ew);
  }

  void setResponseException(folly::exception_wrapper ew) noexcept {
    if (responsePromise_) {
      std::move(*responsePromise_).setException(std::move(ew));
      responsePromise_.reset();
    }
  }

  EventBaseThread clientIoThread_;
  UnixSocket::UniquePtr conn_;
  std::optional<Promise<UnixSocket::Message>> responsePromise_;
};

/**
 * An IXplatLogger that records logged events so tests can assert on the
 * telemetry the privhelper client emits. logEvent() is called from the
 * client's EventBase thread.
 */
class RecordingXplatLogger : public IXplatLogger {
 public:
  void logEvent(std::string_view category, const DynamicEvent& event) override {
    recordedEvents_.wlock()->emplace_back(std::string{category}, event);
    eventRecorded_.post();
  }

  std::vector<std::pair<std::string, DynamicEvent>> getEvents() const {
    return *recordedEvents_.rlock();
  }

  /**
   * Block until at least one event has been recorded. Returns false if none
   * was recorded within the timeout.
   */
  bool waitForEvent(std::chrono::milliseconds timeout) {
    return eventRecorded_.try_wait_for(timeout);
  }

 private:
  folly::Synchronized<std::vector<std::pair<std::string, DynamicEvent>>>
      recordedEvents_;
  // Saturating (multi-post safe), unlike folly::Baton.
  folly::SaturatingSemaphore<true /* MayBlock */> eventRecorded_;
};

class PrivHelperTest : public ::testing::Test {
 protected:
  void SetUp() override {
    File clientConn;
    File serverConn;
    PrivHelperConn::createConnPair(clientConn, serverConn);

    serverThread_ =
        std::thread([this, conn = std::move(serverConn)]() mutable noexcept {
          server_.initPartial(std::move(conn), getuid(), getgid());
          server_.run();
        });
    client_ = createTestPrivHelper(std::move(clientConn));
    client_->setEdenFsEventsLogger(
        std::make_shared<EdenFsEventsLogger>(xplatLogger_));
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
        [&] { client_->attachEventBase(clientIoThread_.getEventBase()); });
  }

  ~PrivHelperTest() override {
    cleanup();
  }

  void cleanup() {
    client_.reset();
    if (serverThread_.joinable()) {
      serverThread_.join();
    }
  }

  std::unique_ptr<PrivHelper> client_;
  PrivHelperThreadedTestServer server_;
  std::thread serverThread_;
  EventBaseThread clientIoThread_;
  std::shared_ptr<RecordingXplatLogger> xplatLogger_{
      std::make_shared<RecordingXplatLogger>()};
};

class PrivHelperFdUnmountTest : public ::testing::Test {
 protected:
  void SetUp() override {
    File clientConn;
    File serverConn;
    PrivHelperConn::createConnPair(clientConn, serverConn);

    serverThread_ =
        std::thread([this, conn = std::move(serverConn)]() mutable noexcept {
          server_.initPartial(std::move(conn), getuid(), getgid());
          server_.run();
        });
    client_ = createTestPrivHelper(std::move(clientConn));
    clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
        [&] { client_->attachEventBase(clientIoThread_.getEventBase()); });
  }

  ~PrivHelperFdUnmountTest() override {
    client_.reset();
    if (serverThread_.joinable()) {
      serverThread_.join();
    }
  }

  std::unique_ptr<PrivHelper> client_;
  PrivHelperFdUnmountTestServer server_;
  std::thread serverThread_;
  EventBaseThread clientIoThread_;
};

class PrivHelperRawProtocolTest : public ::testing::Test {
 protected:
  void SetUp() override {
    File clientConn;
    File serverConn;
    PrivHelperConn::createConnPair(clientConn, serverConn);

    serverThread_ =
        std::thread([this, conn = std::move(serverConn)]() mutable noexcept {
          server_.initPartial(std::move(conn), getuid(), getgid());
          server_.run();
        });
    client_.emplace(std::move(clientConn));
  }

  void TearDown() override {
    client_.reset();
    if (serverThread_.joinable()) {
      serverThread_.join();
    }
  }

  PrivHelperThreadedTestServer server_;
  std::thread serverThread_;
  std::optional<RawPrivHelperClient> client_;
};

TEST_F(PrivHelperRawProtocolTest, legacyMacFuseConfigRequestsAreNoOps) {
  auto timeoutResponse = client_->sendAndRecv(makeLegacyMacFuseConfigRequest(
      1, PrivHelperConn::REQ_SET_DAEMON_TIMEOUT, 60'000'000'000));
  PrivHelperConn::parseEmptyResponse(
      PrivHelperConn::REQ_SET_DAEMON_TIMEOUT, timeoutResponse);

  auto useEdenFsResponse = client_->sendAndRecv(
      makeLegacyMacFuseConfigRequest(2, PrivHelperConn::REQ_SET_USE_EDENFS, 1));
  PrivHelperConn::parseEmptyResponse(
      PrivHelperConn::REQ_SET_USE_EDENFS, useEdenFsResponse);
}

TEST_F(PrivHelperRawProtocolTest, cleanShutdownNotificationIsNotAnswered) {
  client_->send(
      PrivHelperConn::serializeNotifyCleanShutdownRequest(/*xid=*/1, "stop"));

  // Nothing came back for the notification, so this reply is the next one on
  // the wire. Answering a one-way request would make it arrive here instead,
  // and parseEmptyResponse() rejects the mismatched type.
  auto response = client_->sendAndRecv(
      makeLegacyMacFuseConfigRequest(2, PrivHelperConn::REQ_SET_USE_EDENFS, 1));
  PrivHelperConn::parseEmptyResponse(
      PrivHelperConn::REQ_SET_USE_EDENFS, response);
}

TEST_F(PrivHelperTest, fuseMount) {
  auto mountPoint = makeTempDir("bar");
  auto path = mountPoint.path().string();

  // Prepare a promise to use as the result for trying to mount mountPoint
  auto filePromise = server_.setFuseMountResult(path);

  // Call fuseMount() this should return a future that is not ready yet,
  // since we have not fulfilled the promise.
  auto result = client_->fuseMount(path, false, "fuse");
  EXPECT_FALSE(result.isReady());

  // Create a temporary file to respond with
  TemporaryFile tempFile;
  struct stat origStat;
  checkUnixError(
      fstat(tempFile.fd(), &origStat), "failed to stat temporary file");

  // Fulfill the response.
  filePromise.setValue(File(tempFile.fd(), /* ownsFD */ false));

  // The response should complete quickly now.
  auto resultFile = std::move(result).get(1s);

  // The resulting file object should refer to the same underlying file,
  // even though the file descriptor should different since it was passed over
  // a Unix socket.
  EXPECT_NE(tempFile.fd(), resultFile.fd());
  struct stat resultStat;
  checkUnixError(
      fstat(resultFile.fd(), &resultStat), "failed to stat result file");
  EXPECT_EQ(origStat.st_dev, resultStat.st_dev);
  EXPECT_EQ(origStat.st_ino, resultStat.st_ino);

  auto vfsTypes = server_.getRequestedVfsTypes();
  EXPECT_EQ(1, vfsTypes.size());
  EXPECT_EQ("fuse", vfsTypes.at(0));

  // When we shut down the privhelper server it remembers that /foo/bar was
  // unmounted and will try to unmount it.  This will fail since we have not
  // registered a response for the unmount.  This will cause an error message to
  // be logged, but this is fine.
  //
  // We could register a result for the unmount operation here, but seems nice
  // for now to test that the privhelper server gracefully handles the exception
  // from the unmount operation.
}

TEST_F(PrivHelperTest, stalledRequestIsLoggedAndStillSucceeds) {
  auto mountPoint = makeTempDir("bar");
  auto path = mountPoint.path().string();

  client_->setRequestStallThresholdForTest(50ms);

  auto filePromise = server_.setFuseMountResult(path);
  auto result = client_->fuseMount(path, false, "fuse");
  EXPECT_FALSE(result.isReady());

  // Hold the response until the stall watchdog has fired and recorded its
  // event. The watchdog is scheduled for 50ms; the generous timeout only
  // bounds how long we wait on a starved host.
  ASSERT_TRUE(xplatLogger_->waitForEvent(10s));

  TemporaryFile tempFile;
  filePromise.setValue(File(tempFile.fd(), /* ownsFD */ false));

  // The stall watchdog is log-only: the request must still succeed.
  auto resultFile = std::move(result).get(1s);
  EXPECT_GE(resultFile.fd(), 0);

  auto events = xplatLogger_->getEvents();
  ASSERT_EQ(1u, events.size());
  EXPECT_EQ(std::string{xplat_keys::kEventsCategory}, events[0].first);
  const auto& strings = events[0].second.getStringMap();
  EXPECT_EQ(
      "privhelper_request_stall", strings.at(std::string{xplat_keys::kType}));
  EXPECT_EQ("fuse_mount", strings.at(std::string{xplat_keys::kMethod}));
  const auto& doubles = events[0].second.getDoubleMap();
  EXPECT_GE(doubles.at(std::string{xplat_keys::kDuration}), 0.05);
}

TEST_F(PrivHelperTest, fuseMountCustomVfsType) {
  auto mountPoint = makeTempDir("bar");
  auto path = mountPoint.path().string();

  // Prepare a promise to use as the result for trying to mount mountPoint
  auto filePromise = server_.setFuseMountResult(path);

  // Call fuseMount() this should return a future that is not ready yet,
  // since we have not fulfilled the promise.
  auto result = client_->fuseMount(path, false, "fuse.edenfs");
  EXPECT_FALSE(result.isReady());

  // Create a temporary file to respond with
  TemporaryFile tempFile;
  struct stat origStat;
  checkUnixError(
      fstat(tempFile.fd(), &origStat), "failed to stat temporary file");

  // Fulfill the response.
  filePromise.setValue(File(tempFile.fd(), /* ownsFD */ false));

  // The response should complete quickly now.
  auto resultFile = std::move(result).get(1s);

  // The resulting file object should refer to the same underlying file,
  // even though the file descriptor should different since it was passed over
  // a Unix socket.
  EXPECT_NE(tempFile.fd(), resultFile.fd());
  struct stat resultStat;
  checkUnixError(
      fstat(resultFile.fd(), &resultStat), "failed to stat result file");
  EXPECT_EQ(origStat.st_dev, resultStat.st_dev);
  EXPECT_EQ(origStat.st_ino, resultStat.st_ino);

  auto vfsTypes = server_.getRequestedVfsTypes();
  EXPECT_EQ(1, vfsTypes.size());
  EXPECT_EQ("fuse.edenfs", vfsTypes.at(0));

  // When we shut down the privhelper server it remembers that /foo/bar was
  // unmounted and will try to unmount it.  This will fail since we have not
  // registered a response for the unmount.  This will cause an error message to
  // be logged, but this is fine.
  //
  // We could register a result for the unmount operation here, but seems nice
  // for now to test that the privhelper server gracefully handles the exception
  // from the unmount operation.
}

TEST_F(PrivHelperTest, fuseMountPermissions) {
  if (!folly::kIsApple && getuid() != 0) {
    auto path = "/root/bar";
    EXPECT_THROW_RE(
        client_->fuseMount(path, false, "fuse").get(),
        std::exception,
        folly::to<std::string>(
            "std::domain_error: User:",
            getuid(),
            " cannot stat ",
            path,
            ": Permission denied"));
  }
}

TEST_F(PrivHelperTest, fuseMountError) {
  auto tempdir = makeTempDir();
  auto path = tempdir.path().string();
  // Test calling fuseMount() with a mount path that is not registered.
  // This will throw an error in the privhelper server thread.  Make sure the
  // error message is raised in the client correctly.
  EXPECT_THROW_RE(
      client_->fuseMount(path, false, "fuse").get(),
      std::exception,
      fmt::format("no result available for {}", path));
}

TEST_F(PrivHelperTest, multiplePendingFuseMounts) {
  auto abcMountPoint = makeTempDir("abc");
  auto abcPath = abcMountPoint.path().string();
  auto defMountPoint = makeTempDir("def");
  auto defPath = defMountPoint.path().string();
  auto barMountPoint = makeTempDir("bar");
  auto barPath = barMountPoint.path().string();

  // Prepare several promises for various mount points
  auto abcPromise = server_.setFuseMountResult(abcPath);
  auto defPromise = server_.setFuseMountResult(defPath);
  auto barPromise = server_.setFuseMountResult(barPath);

  // Also set up unmount results for when the privhelper tries to unmount these
  // mount points during cleanup.
  server_.setFuseUnmountResult(abcPath).setValue();
  server_.setFuseUnmountResult(defPath).setValue();
  server_.setFuseUnmountResult(barPath).setValue();

  // Make several fuseMount() calls
  auto abcResult = client_->fuseMount(abcPath, false, "fuse");
  auto defResult = client_->fuseMount(defPath, false, "fuse");
  auto foobarResult = client_->fuseMount(barPath, false, "fuse");
  EXPECT_FALSE(abcResult.isReady());
  EXPECT_FALSE(defResult.isReady());
  EXPECT_FALSE(foobarResult.isReady());

  // Fulfill the response promises
  // We fulfill them in a different order than the order of the requests here.
  // This shouldn't affect the behavior of the code.
  TemporaryFile tempFile;
  barPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
  abcPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
  defPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));

  // The responses should be available in the client now.
  auto results =
      folly::collectUnsafe(abcResult, defResult, foobarResult).get(1s);
  (void)results;

  // Destroy the privhelper
  cleanup();

  // All of the unmount results should have been used.
  EXPECT_THAT(server_.getUnusedFuseUnmountResults(), UnorderedElementsAre());
}

TEST_F(PrivHelperTest, bindMounts) {
  auto abcMountPoint = makeTempDir("abc");
  auto abcPath = abcMountPoint.path().string();
  TemporaryFile tempFile;

  boost::filesystem::create_directory(abcMountPoint.path() / "foo");
  boost::filesystem::create_directory(abcMountPoint.path() / "bar");
  boost::filesystem::create_directory(abcMountPoint.path() / "buck-out");
  boost::filesystem::create_directory(
      abcMountPoint.path() / "foo" / "buck-out");
  boost::filesystem::create_directory(
      abcMountPoint.path() / "bar" / "buck-out");

  // Prepare promises for the mount calls
  server_.setFuseMountResult(abcPath).setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(abcPath + "/buck-out").setValue();
  server_.setBindMountResult(abcPath + "/foo/buck-out").setValue();
  server_.setBindMountResult(abcPath + "/bar/buck-out").setValue();

  auto userMountPoint = makeTempDir("user");
  auto userPath = userMountPoint.path().string();

  boost::filesystem::create_directory(userMountPoint.path() / "somerepo");
  boost::filesystem::create_directory(
      userMountPoint.path() / "somerepo" / "buck-out");

  server_.setFuseMountResult(userPath + "/somerepo")
      .setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(userPath + "/somerepo/buck-out").setValue();

  boost::filesystem::create_directory(userMountPoint.path() / "somerepo2");
  server_.setFuseMountResult(userPath + "/somerepo2")
      .setValue(File(tempFile.fd(), false));

  // Prepare promises for the unmount calls
  server_.setFuseUnmountResult(abcPath).setValue();
  server_.setBindUnmountResult(abcPath + "/buck-out").setValue();
  server_.setBindUnmountResult(abcPath + "/foo/buck-out").setValue();
  server_.setBindUnmountResult(abcPath + "/bar/buck-out").setValue();
  server_.setFuseUnmountResult(userPath + "/somerepo").setValue();
  server_.setFuseUnmountResult(userPath + "/somerepo2").setValue();
  // Leave the promise for somerepo/buck-out unfulfilled for now.
  auto somerepoBuckOutUnmountPromise =
      server_.setBindUnmountResult(userPath + "/somerepo/buck-out");

  // Prepare some extra unmount promises that we don't expect to be used,
  // just to verify that cleanup happens as expected.
  server_.setFuseUnmountResult("/never/actually/mounted").setValue();
  server_.setBindUnmountResult("/bind/never/actually/mounted").setValue();

  // Mount everything
  client_->fuseMount(userPath + "/somerepo", false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", userPath + "/somerepo/buck-out")
      .get(1s);

  client_->fuseMount(abcPath, false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/foo/buck-out").get(1s);
  client_->fuseMount(userPath + "/somerepo2", false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/bar/buck-out").get(1s);

  // Manually unmount /somerepo.
  // This will finish even though somerepoBuckOutUnmountPromise is still
  // outstanding because the privhelper and the OS don't care about relative
  // ordering of these two operations.
  auto unmountResult = client_->fuseUnmount(userPath + "/somerepo", {});
  std::move(unmountResult).get(1s);

  // Clean up this promise: no one is waiting on its results, but we just
  // want to make sure that it doesn't generate a BrokenPromise error
  // when the destructors run.
  somerepoBuckOutUnmountPromise.setValue();

  // Now shut down the privhelper.  It should clean up the remaining mount
  // points.  The only leftover results should be the extra ones we
  // intentionally added.
  cleanup();
  EXPECT_THAT(
      server_.getUnusedFuseUnmountResults(),
      UnorderedElementsAre("/never/actually/mounted"));
  EXPECT_THAT(
      server_.getUnusedBindUnmountResults(),
      UnorderedElementsAre("/bind/never/actually/mounted"));
}

TEST_F(PrivHelperTest, bindMountRejectsEscapedMountPath) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific openat2 validation";
#else
  auto mountRoot = makeTempDir("mount-root");
  auto source = makeTempDir("source");
  boost::filesystem::create_directory(mountRoot.path() / "registered");
  boost::filesystem::create_directory(mountRoot.path() / "target");

  const auto registeredPath = (mountRoot.path() / "registered").string();
  const auto escapedPath = registeredPath + "/../target";

  client_->takeoverStartup(registeredPath, {}).get(1s);
  server_.setFuseUnmountResult(registeredPath).setValue();

  server_.setBindMountResult(escapedPath).setValue();
  EXPECT_THROW_RE(
      client_->bindMount(source.path().string(), escapedPath).get(1s),
      std::exception,
      "Invalid cross-device link");

  server_.setBindUnmountResult(escapedPath).setValue();
  EXPECT_THROW_RE(
      client_->bindUnMount(escapedPath).get(1s),
      std::exception,
      "Invalid cross-device link");

#endif
}

TEST_F(PrivHelperTest, bindMountRejectsSymlinkComponent) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific openat2 validation";
#else
  auto mountRoot = makeTempDir("mount-root");
  auto source = makeTempDir("source");
  auto outside = makeTempDir("outside");
  boost::filesystem::create_directory(mountRoot.path() / "registered");
  boost::filesystem::create_directories(outside.path() / "target");
  boost::filesystem::create_directory_symlink(
      outside.path(), mountRoot.path() / "registered" / "link");

  const auto registeredPath = (mountRoot.path() / "registered").string();
  const auto symlinkPath = registeredPath + "/link/target";

  client_->takeoverStartup(registeredPath, {}).get(1s);
  server_.setFuseUnmountResult(registeredPath).setValue();

  server_.setBindMountResult(symlinkPath).setValue();
  EXPECT_THROW_RE(
      client_->bindMount(source.path().string(), symlinkPath).get(1s),
      std::exception,
      "Too many levels of symbolic links");

  server_.setBindUnmountResult(symlinkPath).setValue();
  EXPECT_THROW_RE(
      client_->bindUnMount(symlinkPath).get(1s),
      std::exception,
      "Too many levels of symbolic links");

#endif
}

TEST_F(PrivHelperTest, bindMountRejectsSymlinkHiddenByDotDot) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific openat2 validation";
#else
  auto mountRoot = makeTempDir("mount-root");
  auto source = makeTempDir("source");
  auto outside = makeTempDir("outside");
  boost::filesystem::create_directory(mountRoot.path() / "registered");
  boost::filesystem::create_directory(
      mountRoot.path() / "registered" / "target");
  boost::filesystem::create_directories(outside.path() / "child");
  boost::filesystem::create_directory(outside.path() / "target");
  boost::filesystem::create_directory_symlink(
      outside.path() / "child", mountRoot.path() / "registered" / "link");

  const auto registeredPath = (mountRoot.path() / "registered").string();
  const auto symlinkDotDotPath = registeredPath + "/link/../target";

  client_->takeoverStartup(registeredPath, {}).get(1s);
  server_.setFuseUnmountResult(registeredPath).setValue();

  server_.setBindMountResult(symlinkDotDotPath).setValue();
  EXPECT_THROW_RE(
      client_->bindMount(source.path().string(), symlinkDotDotPath).get(1s),
      std::exception,
      "Too many levels of symbolic links");

  server_.setBindUnmountResult(symlinkDotDotPath).setValue();
  EXPECT_THROW_RE(
      client_->bindUnMount(symlinkDotDotPath).get(1s),
      std::exception,
      "Too many levels of symbolic links");

#endif
}

TEST_F(PrivHelperTest, bindMountAllowsSymlinkAncestorOfRegisteredMount) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific openat2 validation";
#else
  auto realRoot = makeTempDir("real-root");
  auto linkRoot = makeTempDir("link-root");
  auto source = makeTempDir("source");
  boost::filesystem::create_directories(realRoot.path() / "registered");
  boost::filesystem::create_directories(
      realRoot.path() / "registered" / "redirected");
  boost::filesystem::create_directory_symlink(
      realRoot.path(), linkRoot.path() / "link");

  const auto registeredPath =
      (linkRoot.path() / "link" / "registered").string();
  const auto bindPath = registeredPath + "/redirected";

  client_->takeoverStartup(registeredPath, {}).get(1s);
  server_.setFuseUnmountResult(registeredPath).setValue();
  server_.setBindMountResult(bindPath).setValue();

  client_->bindMount(source.path().string(), bindPath).get(1s);
#endif
}

TEST_F(
    PrivHelperTest,
    bindMountRejectsRetargetedSymlinkAncestorOfRegisteredMount) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific openat2 validation";
#else
  auto realRoot = makeTempDir("real-root");
  auto alternateRoot = makeTempDir("alternate-root");
  auto linkRoot = makeTempDir("link-root");
  auto source = makeTempDir("source");
  boost::filesystem::create_directories(realRoot.path() / "registered");
  boost::filesystem::create_directories(
      alternateRoot.path() / "registered" / "redirected");
  boost::filesystem::create_directory_symlink(
      realRoot.path(), linkRoot.path() / "link");

  const auto registeredPath =
      (linkRoot.path() / "link" / "registered").string();
  const auto bindPath = registeredPath + "/redirected";

  client_->takeoverStartup(registeredPath, {}).get(1s);
  server_.setFuseUnmountResult(registeredPath).setValue();

  boost::filesystem::remove(linkRoot.path() / "link");
  boost::filesystem::create_directory_symlink(
      alternateRoot.path(), linkRoot.path() / "link");

  server_.setBindMountResult(bindPath).setValue();
  EXPECT_THROW_RE(
      client_->bindMount(source.path().string(), bindPath).get(1s),
      std::exception,
      "No such file or directory");
#endif
}

TEST_F(PrivHelperFdUnmountTest, bindUnmountAllowsAlreadyUnmountedTarget) {
#ifdef __APPLE__
  GTEST_SKIP() << "Linux-specific procfd bind unmount validation";
#else
  auto mountRoot = makeTempDir("mount-root");
  boost::filesystem::create_directory(mountRoot.path() / "registered");
  boost::filesystem::create_directory(
      mountRoot.path() / "registered" / "not-mounted");

  const auto registeredPath = (mountRoot.path() / "registered").string();
  const auto notMountedPath = registeredPath + "/not-mounted";

  client_->takeoverStartup(registeredPath, {}).get(1s);

  client_->bindUnMount(notMountedPath).get(1s);
  EXPECT_FALSE(server_.insecureBindUnmountCalled());
#endif
}

TEST_F(PrivHelperTest, takeoverShutdown) {
  auto abcMountPoint = makeTempDir("abc");
  auto abcPath = abcMountPoint.path().string();
  TemporaryFile tempFile;

  boost::filesystem::create_directory(abcMountPoint.path() / "foo");
  boost::filesystem::create_directory(abcMountPoint.path() / "bar");
  boost::filesystem::create_directory(abcMountPoint.path() / "buck-out");
  boost::filesystem::create_directory(
      abcMountPoint.path() / "foo" / "buck-out");
  boost::filesystem::create_directory(
      abcMountPoint.path() / "bar" / "buck-out");

  // Prepare promises for the mount calls
  server_.setFuseMountResult(abcPath).setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(abcPath + "/buck-out").setValue();
  server_.setBindMountResult(abcPath + "/foo/buck-out").setValue();
  server_.setBindMountResult(abcPath + "/bar/buck-out").setValue();

  auto userMountPoint = makeTempDir("user");
  auto userPath = userMountPoint.path().string();

  boost::filesystem::create_directory(userMountPoint.path() / "somerepo");

  server_.setFuseMountResult(userPath + "/somerepo")
      .setValue(File(tempFile.fd(), false));

  boost::filesystem::create_directory(userMountPoint.path() / "somerepo2");
  boost::filesystem::create_directory(
      userMountPoint.path() / "somerepo2" / "buck-out");
  server_.setFuseMountResult(userPath + "/somerepo2")
      .setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(userPath + "/somerepo2/buck-out").setValue();

  // Set up unmount promises
  server_.setFuseUnmountResult(abcPath).setValue();
  server_.setBindUnmountResult(abcPath + "/buck-out").setValue();
  server_.setBindUnmountResult(abcPath + "/foo/buck-out").setValue();
  server_.setBindUnmountResult(abcPath + "/bar/buck-out").setValue();
  server_.setFuseUnmountResult(userPath + "/somerepo").setValue();
  server_.setFuseUnmountResult(userPath + "/somerepo2").setValue();
  server_.setBindUnmountResult(userPath + "/somerepo2/buck-out").setValue();

  // Mount everything
  client_->fuseMount(abcPath, false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/foo/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/bar/buck-out").get(1s);
  client_->fuseMount(userPath + "/somerepo", false, "fuse").get(1s);
  client_->fuseMount(userPath + "/somerepo2", false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", userPath + "/somerepo2/buck-out")
      .get(1s);

  // Indicate that /mnt/abc and /mnt/somerepo are being taken over.
  client_->takeoverShutdown(abcPath).get(1s);
  client_->takeoverShutdown(userPath + "/somerepo").get(1s);

  // Destroy the privhelper.
  // /mnt/somerepo2 should be unmounted, but /mnt/abc and /mnt/somerepo
  // should not be.
  cleanup();

  EXPECT_THAT(
      server_.getUnusedFuseUnmountResults(),
      UnorderedElementsAre(abcPath, userPath + "/somerepo"));
  EXPECT_THAT(
      server_.getUnusedBindUnmountResults(),
      UnorderedElementsAre(
          abcPath + "/buck-out",
          abcPath + "/foo/buck-out",
          abcPath + "/bar/buck-out"));
}

TEST_F(PrivHelperTest, takeoverStartup) {
  auto abcMountPoint = makeTempDir("abc");
  auto abcPath = abcMountPoint.path().string();
  TemporaryFile tempFile;

  boost::filesystem::create_directories(
      abcMountPoint.path() / "foo" / "buck-out");
  boost::filesystem::create_directories(
      abcMountPoint.path() / "xyz" / "test" / "buck-out");

  // Indicate that we are taking over some mount points.
  client_
      ->takeoverStartup(
          abcPath, {abcPath + "/foo/buck-out", abcPath + "/xyz/test/buck-out"})
      .get(1s);

  auto myrepoMountPoint = makeTempDir("myrepo");
  auto myrepoPath = myrepoMountPoint.path().string();
  client_->takeoverStartup(myrepoPath, {}).get(1s);

  auto repoXMountPoint = makeTempDir("repo_x");
  auto repoXPath = repoXMountPoint.path().string();
  client_->takeoverStartup(repoXPath, {repoXPath + "/y"}).get(1s);

  // Manually mount one other mount point.
  auto xyzMountPoint = makeTempDir("xyz");
  auto xyzPath = xyzMountPoint.path().string();
  boost::filesystem::create_directory(xyzMountPoint.path() / "buck-out");
  server_.setFuseMountResult(xyzPath).setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(xyzPath + "/buck-out").setValue();
  client_->fuseMount(xyzPath, false, "fuse").get(1s);
  client_->bindMount("/bind/mount/source", xyzPath + "/buck-out").get(1s);

  // Manually unmount /mnt/repo_x
  server_.setFuseUnmountResult(repoXPath).setValue();
  server_.setBindUnmountResult(repoXPath + "/y").setValue();
  client_->fuseUnmount(repoXPath, {}).get(1s);
  EXPECT_THAT(server_.getUnusedFuseUnmountResults(), UnorderedElementsAre());
  EXPECT_THAT(server_.getUnusedBindUnmountResults(), UnorderedElementsAre());

  // Re-register the unmount results for repo_x just to confirm that they are
  // not reused on shutdown.
  server_.setFuseUnmountResult(repoXPath).setValue();
  server_.setBindUnmountResult(repoXPath + "/y").setValue();

  // Register results for the other unmount operations that should occur.
  server_.setFuseUnmountResult(abcPath).setValue();
  server_.setBindUnmountResult(abcPath + "/foo/buck-out").setValue();
  server_.setBindUnmountResult(abcPath + "/xyz/test/buck-out").setValue();
  server_.setFuseUnmountResult(xyzPath).setValue();
  server_.setBindUnmountResult(xyzPath + "/buck-out").setValue();
  server_.setFuseUnmountResult(myrepoPath).setValue();

  // Shut down the privhelper.  It should unmount the registered mount points.
  cleanup();
  EXPECT_THAT(
      server_.getUnusedFuseUnmountResults(), UnorderedElementsAre(repoXPath));
  EXPECT_THAT(
      server_.getUnusedBindUnmountResults(),
      UnorderedElementsAre(repoXPath + "/y"));
}

TEST_F(PrivHelperTest, detachEventBase) {
  auto barMountPoint = makeTempDir("bar");
  auto barPath = barMountPoint.path().string();
  // Perform one call using the current EventBase
  TemporaryFile tempFile;
  auto filePromise = server_.setFuseMountResult(barPath);
  auto result = client_->fuseMount(barPath, false, "fuse");
  EXPECT_FALSE(result.isReady());
  filePromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
  auto resultFile = std::move(result).get(1s);

  // Detach the PrivHelper from the clientIoThread_'s EventBase, and perform a
  // call using a separate local EventBase
  clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client_->detachEventBase(); });

  {
    EventBase evb;
    client_->attachEventBase(&evb);

    auto newMountPoint = makeTempDir("new");
    auto newPath = newMountPoint.path().string();

    filePromise = server_.setFuseMountResult(newPath);
    server_.setFuseUnmountResult(newPath).setValue();
    result = client_->fuseMount(newPath, false, "fuse");
    // The result should not be immediately ready since we have not fulfilled
    // the promise yet.  It will only be ready if something unexpected failed.
    if (result.isReady()) {
      ADD_FAILURE() << "unmount request was immediately ready";
      // Call get() so it will throw if the command failed.
      std::move(result).get();
      return;
    }

    bool success = false;
    std::move(result)
        .thenValue([&success](folly::File) { success = true; })
        .ensure([&evb] { evb.terminateLoopSoon(); });

    filePromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
    evb.loopForever();
    EXPECT_TRUE(success);

    // The PrivHelper will be automatically detached from this EventBase
    // when it goes out of scope and is destroyed.
  }

  // Re-attach the PrivHelper to the clientIoThread_'s EventBase
  clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client_->attachEventBase(clientIoThread_.getEventBase()); });

  // Perform another call with the clientIoThread_ EventBase
  auto unmountPromise = server_.setFuseUnmountResult(barPath);
  auto unmountResult = client_->fuseUnmount(barPath, {});
  EXPECT_FALSE(unmountResult.isReady());
  unmountPromise.setValue();
  std::move(unmountResult).get(1s);
}

TEST_F(PrivHelperTest, setLogFile) {
  // Call setLogFile()
  TemporaryFile tempFile0;
  client_->setLogFile(File{tempFile0.fd(), /* ownsFD */ false}).get(1s);

  // Detach from the clientIoThread_ and call all setLogFileBlocking()
  TemporaryFile tempFile1;
  clientIoThread_.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client_->detachEventBase(); });
  client_->setLogFileBlocking(File{tempFile1.fd(), /* ownsFD */ false});

  // Confirm that the server received both requests
  auto logFiles = server_.getLogFileRequests();
  ASSERT_EQ(2, logFiles.size());

  struct stat s1;
  folly::checkUnixError(fstat(logFiles[0].fd(), &s1));
  struct stat s2;
  folly::checkUnixError(fstat(tempFile0.fd(), &s2));
  EXPECT_EQ(s1.st_dev, s2.st_dev);
  EXPECT_EQ(s1.st_ino, s2.st_ino);

  folly::checkUnixError(fstat(logFiles[1].fd(), &s1));
  folly::checkUnixError(fstat(tempFile1.fd(), &s2));
  EXPECT_EQ(s1.st_dev, s2.st_dev);
  EXPECT_EQ(s1.st_ino, s2.st_ino);
}

TEST(PrivHelperSessionTest, detachesFromParentProcessGroup) {
  // The privhelper binary calls detachFromParentProcessGroup() at startup
  // so that killing the process group it was spawned into (as agent
  // command runners do when cleaning up an `eden restart` invocation)
  // cannot take the privhelper down with it. Verify in a forked child
  // that the call moves the process into its own session and group.
  // The child only makes raw syscalls, so forking with test threads
  // running is safe.
  pid_t childPid = fork();
  folly::checkUnixError(childPid, "fork failed");
  if (childPid == 0) {
    if (getpgid(0) == getpid()) {
      // The child must start out in its parent's process group for this
      // test to prove anything.
      _exit(2);
    }
    detachFromParentProcessGroup();
    if (getpgid(0) != getpid()) {
      _exit(3);
    }
    if (getsid(0) != getpid()) {
      _exit(4);
    }
    _exit(0);
  }
  int status = 0;
  folly::checkUnixError(waitpid(childPid, &status, 0), "waitpid failed");
  ASSERT_TRUE(WIFEXITED(status));
  EXPECT_EQ(0, WEXITSTATUS(status));
}

TEST_F(PrivHelperTest, cleanShutdownNotificationLeavesTheConnectionUsable) {
  client_->notifyCleanShutdown("stop");

  // A privhelper that does not act on this request -- every Linux one, and any
  // build too old to know the type -- must still not reply to it. A later
  // request completing proves nothing crashed and the stream is intact.
  EXPECT_EQ(getpid(), std::move(client_->getServerPid()).get(1s));
}

TEST(
    PrivHelperClientLifetime,
    destroyingTheClientLeavesRequestsQueuedOnItsEventBaseSafeToRun) {
  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);

  PrivHelperThreadedTestServer server;
  std::thread serverThread(
      [&server, conn = std::move(serverConn)]() mutable noexcept {
        server.initPartial(std::move(conn), getuid(), getgid());
        server.run();
      });

  // Declared before the client so that it outlives it, leaving the request
  // queued against a client that is already gone.
  EventBase eventBase;
  auto pid = Future<pid_t>::makeEmpty();
  {
    auto client = createTestPrivHelper(std::move(clientConn));
    client->attachEventBase(&eventBase);
    pid = client->getServerPid();
  }

  // Destroying the client does not drain the queue, and does not have to: the
  // request holds the session alive, so running it late is defined.
  EXPECT_EQ(1u, eventBase.getNotificationQueueSize());

  eventBase.loopOnce(EVLOOP_NONBLOCK);
  EXPECT_EQ(0u, eventBase.getNotificationQueueSize());
  EXPECT_THROW_RE(
      std::move(pid).get(1s),
      std::runtime_error,
      "cannot send new requests on closed privhelper connection");

  serverThread.join();
}

TEST(PrivHelperConnectionLossTest, serverDeathFailsRequestsWithoutDeadlock) {
  // This test models the privhelper process dying (e.g. killed under memory
  // pressure) while the daemon is running: the server end of the socket
  // closes and the client sees EOF.
  EventBaseThread ioThread;
  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);
  auto client = createTestPrivHelper(std::move(clientConn));
  ioThread.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client->attachEventBase(ioThread.getEventBase()); });

  // Issue a request that the server never answers, and make sure it has
  // been written before the connection drops, so this test pins the EOF
  // path rather than the send-failure path covered below.
  auto pending = client->fuseUnmount("/never/answered", {});
  ioThread.getEventBase()->runInEventBaseThreadAndWait([] {});

  // The privhelper process dies.
  serverConn.close();

  // The pending request fails with the connection error rather than
  // hanging. (folly::FutureTimeout is a std::logic_error, so this assertion
  // also proves the future was actually fulfilled.)
  EXPECT_THROW(std::move(pending).get(5s), std::runtime_error);

  // New requests fail fast instead of queueing against a dead connection.
  EXPECT_THROW(
      client->fuseUnmount("/other/mount", {}).get(5s), std::runtime_error);

  // The closed connection no longer has a file descriptor to report.
  EXPECT_EQ(-1, client->getRawClientFd());

  // The EventBase thread survived processing the EOF.
  folly::Baton<> alive;
  ioThread.getEventBase()->runInEventBaseThread([&] { alive.post(); });
  EXPECT_TRUE(alive.try_wait_for(5s));
}

TEST(PrivHelperConnectionLossTest, sendFailureFailsRequestsWithoutDeadlock) {
  // Same failure family as above, but through the other entry point: the
  // send itself fails synchronously while the connection still looks open,
  // which invokes the error callbacks from inside send().
#ifdef __APPLE__
  // On macOS, shutting down the peer's receive side does not make sends
  // fail with EPIPE (they are silently accepted until the socket is fully
  // closed), so this test cannot trigger the send-failure path there.
  GTEST_SKIP() << "shutdown(SHUT_RD) does not fail peer sends on macOS";
#else
  EventBaseThread ioThread;
  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);
  auto client = createTestPrivHelper(std::move(clientConn));
  ioThread.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client->attachEventBase(ioThread.getEventBase()); });

  // Shut down only the server's receiving side: the client never sees EOF,
  // but its next send fails with EPIPE.
  folly::checkUnixError(
      ::shutdown(serverConn.fd(), SHUT_RD), "shutdown failed");

  auto pending = client->fuseUnmount("/never/answered", {});
  EXPECT_THROW(std::move(pending).get(5s), std::runtime_error);
  EXPECT_THROW(
      client->fuseUnmount("/other/mount", {}).get(5s), std::runtime_error);

  folly::Baton<> alive;
  ioThread.getEventBase()->runInEventBaseThread([&] { alive.post(); });
  EXPECT_TRUE(alive.try_wait_for(5s));
#endif // !__APPLE__
}

TEST(PrivHelperConnectionLossTest, unexpectedExitLogsOneEvent) {
  EventBaseThread ioThread;
  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);
  auto client = createTestPrivHelper(std::move(clientConn));
  auto recorder = std::make_shared<RecordingXplatLogger>();
  client->setEdenFsEventsLogger(std::make_shared<EdenFsEventsLogger>(recorder));
  ioThread.getEventBase()->runInEventBaseThreadAndWait(
      [&] { client->attachEventBase(ioThread.getEventBase()); });

  auto pending = client->fuseUnmount("/never/answered", {});
  ioThread.getEventBase()->runInEventBaseThreadAndWait([] {});

  // Drain the request from the server side before closing: closing a
  // socket with unread data produces ECONNRESET on the client instead of
  // a clean EOF.
  char buf[4096];
  while (recv(serverConn.fd(), buf, sizeof(buf), MSG_DONTWAIT) > 0) {
  }

  // The privhelper process dies.
  serverConn.close();
  EXPECT_THROW(std::move(pending).get(5s), std::runtime_error);

  // Exactly one privhelper_exit event is logged, even though tearing down
  // the connection triggers multiple socket callbacks (EOF, socket closed).
  auto events = recorder->getEvents();
  ASSERT_EQ(1ul, events.size());
  const auto& strings = events[0].second.getStringMap();
  EXPECT_EQ("privhelper_exit", strings.at("type"));
  EXPECT_EQ("eof", strings.at("reason"));
}

TEST(PrivHelperConnectionLossTest, cleanShutdownLogsNoEvent) {
  EventBaseThread ioThread;
  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);
  auto recorder = std::make_shared<RecordingXplatLogger>();
  {
    auto client = createTestPrivHelper(std::move(clientConn));
    client->setEdenFsEventsLogger(
        std::make_shared<EdenFsEventsLogger>(recorder));
    ioThread.getEventBase()->runInEventBaseThreadAndWait(
        [&] { client->attachEventBase(ioThread.getEventBase()); });
    ioThread.getEventBase()->runInEventBaseThreadAndWait(
        [&] { client->detachEventBase(); });
  }

  // Locally-initiated teardown is not an unexpected privhelper exit.
  EXPECT_EQ(0ul, recorder->getEvents().size());
}

#ifdef __APPLE__

/** Exposes the sentinel reader and the state it consults, all protected. */
class PrivHelperSentinelTestServer : public PrivHelperServer {
 public:
  using PrivHelperServer::readRelaunchCommand;
  using PrivHelperServer::restartConfig_;
  using PrivHelperServer::uid_;
};

/**
 * The sentinel is written by the daemon's unprivileged user and read by a root
 * privhelper, so these cases are all about what a replaced file can do to the
 * reader rather than about ordinary parse errors.
 */
class PrivHelperSentinelTest : public ::testing::Test {
 protected:
  void SetUp() override {
    dir_ = std::make_unique<TemporaryDirectory>("edenfs_sentinel");
    server_.restartConfig_ = makeRestartArgs(sentinelPath());
    server_.uid_ = getuid();
  }

  std::string sentinelPath() const {
    return (dir_->path() / "sentinel").string();
  }

  /** Owned by us and only ours to write, as the daemon writes it. */
  void writeSentinel(const std::string& contents) {
    ASSERT_TRUE(folly::writeFile(contents, sentinelPath().c_str()));
    checkUnixError(::chmod(sentinelPath().c_str(), 0600));
  }

  PrivHelperSentinelTestServer server_;
  std::unique_ptr<TemporaryDirectory> dir_;
};

TEST_F(PrivHelperSentinelTest, readsTheCommandAndEnvironment) {
  writeSentinel(makeSentinelContents());

  const auto command = server_.readRelaunchCommand();
  ASSERT_TRUE(command.has_value());
  EXPECT_EQ(kSentinelArgv, command->argv);
  EXPECT_THAT(
      command->env,
      UnorderedElementsAre(
          std::pair<std::string, std::string>{"PATH", "/usr/bin"},
          std::pair<std::string, std::string>{"HOME", "/home/test"}));
}

TEST_F(PrivHelperSentinelTest, rejectsASymlink) {
  const auto target = (dir_->path() / "target").string();
  ASSERT_TRUE(folly::writeFile(makeSentinelContents(), target.c_str()));
  checkUnixError(::symlink(target.c_str(), sentinelPath().c_str()));

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAFifo) {
  // Without the regular-file check, opening this would block a root process
  // that still owes the mounts a cleanup.
  checkUnixError(::mkfifo(sentinelPath().c_str(), 0600));

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsASentinelOwnedByAnotherUser) {
  writeSentinel(makeSentinelContents());
  server_.uid_ = getuid() + 1;

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAGroupWritableSentinel) {
  writeSentinel(makeSentinelContents());
  checkUnixError(::chmod(sentinelPath().c_str(), 0660));

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAnOversizedFile) {
  writeSentinel(std::string(2 * 1024 * 1024, 'x'));

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAnEmptyFile) {
  writeSentinel("");

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAMissingFile) {
  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsASentinelFromAnotherGeneration) {
  writeSentinel(makeSentinelContents(kSentinelNonce + 1));

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsASentinelWithNoNonce) {
  writeSentinel(makeSentinelContentsWithoutNonce());

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

TEST_F(PrivHelperSentinelTest, rejectsAConfigurationWithNoNonce) {
  writeSentinel(makeSentinelContentsWithoutNonce());
  server_.restartConfig_->sentinelNonce = 0;

  EXPECT_FALSE(server_.readRelaunchCommand().has_value());
}

#endif // __APPLE__

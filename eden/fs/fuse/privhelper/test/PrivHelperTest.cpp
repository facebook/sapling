/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventBaseThread.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <chrono>
#include <unordered_map>

#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/fuse/privhelper/PrivHelperImpl.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/fuse/privhelper/test/PrivHelperTestServer.h"

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
    data_.wlock()->fuseMountResults.emplace(path.str(), promise.getFuture());
    return promise;
  }

  Promise<Unit> setFuseUnmountResult(StringPiece path) {
    Promise<Unit> promise;
    data_.wlock()->fuseUnmountResults.emplace(path.str(), promise.getFuture());
    return promise;
  }

  Promise<Unit> setBindMountResult(StringPiece path) {
    Promise<Unit> promise;
    data_.wlock()->bindMountResults.emplace(path.str(), promise.getFuture());
    return promise;
  }

  Promise<Unit> setBindUnmountResult(StringPiece path) {
    Promise<Unit> promise;
    data_.wlock()->bindUnmountResults.emplace(path.str(), promise.getFuture());
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

 private:
  struct Data {
    std::unordered_map<string, Future<File>> fuseMountResults;
    std::unordered_map<string, Future<Unit>> fuseUnmountResults;
    std::unordered_map<string, Future<Unit>> bindMountResults;
    std::unordered_map<string, Future<Unit>> bindUnmountResults;
    std::vector<File> logFiles;
  };

  template <typename T>
  folly::Future<T> getResultFuture(
      std::unordered_map<string, Future<T>>& map,
      StringPiece path) {
    auto iter = map.find(path.str());
    if (iter == map.end()) {
      throw std::runtime_error(
          folly::to<string>("no result available for ", path));
    }
    auto future = std::move(iter->second);
    map.erase(iter);
    return future;
  }

  template <typename T>
  std::vector<string> getUnusedResults(
      const std::unordered_map<std::string, Future<T>>& map) {
    std::vector<string> results;
    for (const auto& entry : map) {
      results.push_back(entry.first);
    }
    return results;
  }

  folly::File fuseMount(const char* mountPath) override {
    auto future = getResultFuture(data_.wlock()->fuseMountResults, mountPath);
    return std::move(future).get(1s);
  }

  void fuseUnmount(const char* mountPath) override {
    auto future = getResultFuture(data_.wlock()->fuseUnmountResults, mountPath);
    std::move(future).get(1s);
  }

  void bindMount(const char* /* clientPath */, const char* mountPath) override {
    auto future = getResultFuture(data_.wlock()->bindMountResults, mountPath);
    std::move(future).get(1s);
  }

  void bindUnmount(const char* mountPath) override {
    auto future = getResultFuture(data_.wlock()->bindUnmountResults, mountPath);
    std::move(future).get(1s);
  }

  void setLogFile(folly::File&& logFile) override {
    auto data = data_.wlock();
    data->logFiles.push_back(std::move(logFile));
  }

  folly::Synchronized<Data> data_;
};

class PrivHelperTest : public ::testing::Test {
 protected:
  void SetUp() override {
    File clientConn;
    File serverConn;
    PrivHelperConn::createConnPair(clientConn, serverConn);

    serverThread_ =
        std::thread([ this, conn = std::move(serverConn) ]() mutable noexcept {
          server_.initPartial(std::move(conn), getuid(), getgid());
          server_.run();
        });
    client_ = createTestPrivHelper(std::move(clientConn));
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
};

TEST_F(PrivHelperTest, fuseMount) {
  // Prepare a promise to use as the result for trying to mount "/foo/bar"
  auto filePromise = server_.setFuseMountResult("/foo/bar");

  // Call fuseMount() this should return a future that is not ready yet,
  // since we have not fulfilled the promise.
  auto result = client_->fuseMount("/foo/bar");
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

  // When we shut down the privhelper server it remembers that /foo/bar was
  // unmounted and will try to unmount it.  This will fail since we have not
  // registered a response for the unmount.  This will cause an error message to
  // be logged, but this is fine.
  //
  // We could register a result for the unmount operation here, but seems nice
  // for now to test that the privhelper server gracefully handles the exception
  // from the unmount operation.
}

TEST_F(PrivHelperTest, fuseMountError) {
  // Test calling fuseMount() with a mount path that is not registered.
  // This will throw an error in the privhelper server thread.  Make sure the
  // error message is raised in the client correctly.
  EXPECT_THROW_RE(
      client_->fuseMount("/foo/bar").get(),
      std::exception,
      "no result available for /foo/bar");
}

TEST_F(PrivHelperTest, multiplePendingFuseMounts) {
  // Prepare several promises for various mount points
  auto abcPromise = server_.setFuseMountResult("/mnt/abc");
  auto defPromise = server_.setFuseMountResult("/mnt/def");
  auto foobarPromise = server_.setFuseMountResult("/foo/bar");

  // Also set up unmount results for when the privhelper tries to unmount these
  // mount points during cleanup.
  server_.setFuseUnmountResult("/mnt/abc").setValue();
  server_.setFuseUnmountResult("/mnt/def").setValue();
  server_.setFuseUnmountResult("/foo/bar").setValue();

  // Make several fuseMount() calls
  auto abcResult = client_->fuseMount("/mnt/abc");
  auto defResult = client_->fuseMount("/mnt/def");
  auto foobarResult = client_->fuseMount("/foo/bar");
  EXPECT_FALSE(abcResult.isReady());
  EXPECT_FALSE(defResult.isReady());
  EXPECT_FALSE(foobarResult.isReady());

  // Fulfill the response promises
  // We fulfill them in a different order than the order of the requests here.
  // This shouldn't affect the behavior of the code.
  TemporaryFile tempFile;
  foobarPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
  abcPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));
  defPromise.setValue(File(tempFile.fd(), /* ownsFD */ false));

  // The responses should be available in the client now.
  auto results = folly::collect(abcResult, defResult, foobarResult).get(1s);
  (void)results;

  // Destroy the privhelper
  cleanup();

  // All of the unmount results should have been used.
  EXPECT_THAT(server_.getUnusedFuseUnmountResults(), UnorderedElementsAre());
}

TEST_F(PrivHelperTest, bindMounts) {
  TemporaryFile tempFile;

  // Prepare promises for the mount calls
  server_.setFuseMountResult("/mnt/abc").setValue(File(tempFile.fd(), false));
  server_.setBindMountResult("/mnt/abc/buck-out").setValue();
  server_.setBindMountResult("/mnt/abc/foo/buck-out").setValue();
  server_.setBindMountResult("/mnt/abc/bar/buck-out").setValue();

  server_.setFuseMountResult("/data/users/foo/somerepo")
      .setValue(File(tempFile.fd(), false));
  server_.setBindMountResult("/data/users/foo/somerepo/buck-out").setValue();

  server_.setFuseMountResult("/data/users/foo/somerepo2")
      .setValue(File(tempFile.fd(), false));

  // Prepare promises for the unmount calls
  server_.setFuseUnmountResult("/mnt/abc").setValue();
  server_.setBindUnmountResult("/mnt/abc/buck-out").setValue();
  server_.setBindUnmountResult("/mnt/abc/foo/buck-out").setValue();
  server_.setBindUnmountResult("/mnt/abc/bar/buck-out").setValue();
  server_.setFuseUnmountResult("/data/users/foo/somerepo").setValue();
  server_.setFuseUnmountResult("/data/users/foo/somerepo2").setValue();
  // Leave the promise for somerepo/buck-out unfulfilled for now
  auto somerepoBuckOutUnmountPromise =
      server_.setBindUnmountResult("/data/users/foo/somerepo/buck-out");

  // Prepare some extra unmount promises that we don't expect to be used,
  // just to verify that cleanup happens as expected.
  server_.setFuseUnmountResult("/never/actually/mounted").setValue();
  server_.setBindUnmountResult("/bind/never/actually/mounted").setValue();

  // Mount everything
  client_->fuseMount("/data/users/foo/somerepo").get(1s);
  client_->bindMount("/bind/mount/source", "/data/users/foo/somerepo/buck-out")
      .get(1s);

  client_->fuseMount("/mnt/abc").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/foo/buck-out").get(1s);
  client_->fuseMount("/data/users/foo/somerepo2").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/bar/buck-out").get(1s);

  // Manually unmount /data/users/foo/somerepo
  // This shouldn't finish until the bind unmount completes.
  auto unmountResult = client_->fuseUnmount("/data/users/foo/somerepo");
  /* sleep override */ std::this_thread::sleep_for(20ms);
  EXPECT_FALSE(unmountResult.isReady());
  // Fulfilling the bind unmount promise for the buck-out bind mount should
  // allow the overall unmount operation to complete.
  somerepoBuckOutUnmountPromise.setValue();
  std::move(unmountResult).get(1s);

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

TEST_F(PrivHelperTest, takeoverShutdown) {
  TemporaryFile tempFile;

  // Set up mount promises
  server_.setFuseMountResult("/mnt/abc").setValue(File(tempFile.fd(), false));
  server_.setBindMountResult("/mnt/abc/buck-out").setValue();
  server_.setBindMountResult("/mnt/abc/foo/buck-out").setValue();
  server_.setBindMountResult("/mnt/abc/bar/buck-out").setValue();

  server_.setFuseMountResult("/mnt/somerepo")
      .setValue(File(tempFile.fd(), false));

  server_.setFuseMountResult("/mnt/somerepo2")
      .setValue(File(tempFile.fd(), false));
  server_.setBindMountResult("/mnt/somerepo2/buck-out").setValue();

  // Set up unmount promises
  server_.setFuseUnmountResult("/mnt/abc").setValue();
  server_.setBindUnmountResult("/mnt/abc/buck-out").setValue();
  server_.setBindUnmountResult("/mnt/abc/foo/buck-out").setValue();
  server_.setBindUnmountResult("/mnt/abc/bar/buck-out").setValue();
  server_.setFuseUnmountResult("/mnt/somerepo").setValue();
  server_.setFuseUnmountResult("/mnt/somerepo2").setValue();
  server_.setBindUnmountResult("/mnt/somerepo2/buck-out").setValue();

  // Mount everything
  client_->fuseMount("/mnt/abc").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/foo/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/abc/bar/buck-out").get(1s);
  client_->fuseMount("/mnt/somerepo").get(1s);
  client_->fuseMount("/mnt/somerepo2").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/somerepo2/buck-out").get(1s);

  // Indicate that /mnt/abc and /mnt/somerepo are being taken over.
  client_->fuseTakeoverShutdown("/mnt/abc").get(1s);
  client_->fuseTakeoverShutdown("/mnt/somerepo").get(1s);

  // Destroy the privhelper.
  // /mnt/somerepo2 should be unmounted, but /mnt/abc and /mnt/somerepo
  // should not be.
  cleanup();

  EXPECT_THAT(
      server_.getUnusedFuseUnmountResults(),
      UnorderedElementsAre("/mnt/abc", "/mnt/somerepo"));
  EXPECT_THAT(
      server_.getUnusedBindUnmountResults(),
      UnorderedElementsAre(
          "/mnt/abc/buck-out",
          "/mnt/abc/foo/buck-out",
          "/mnt/abc/bar/buck-out"));
}

TEST_F(PrivHelperTest, takeoverStartup) {
  TemporaryFile tempFile;

  // Indicate that we are taking over some mount points.
  client_
      ->fuseTakeoverStartup(
          "/mnt/abc", {"/mnt/abc/foo/buck-out", "/mnt/abc/xyz/test/buck-out"})
      .get(1s);
  client_->fuseTakeoverStartup("/data/users/johndoe/myrepo", {}).get(1s);
  client_->fuseTakeoverStartup("/mnt/repo_x", {"/mnt/repo_x/y"}).get(1s);

  // Manually mount one other mount point.
  server_.setFuseMountResult("/mnt/xyz").setValue(File(tempFile.fd(), false));
  server_.setBindMountResult("/mnt/xyz/buck-out").setValue();
  client_->fuseMount("/mnt/xyz").get(1s);
  client_->bindMount("/bind/mount/source", "/mnt/xyz/buck-out").get(1s);

  // Manually unmount /mnt/repo_x
  server_.setFuseUnmountResult("/mnt/repo_x").setValue();
  server_.setBindUnmountResult("/mnt/repo_x/y").setValue();
  client_->fuseUnmount("/mnt/repo_x").get(1s);
  EXPECT_THAT(server_.getUnusedFuseUnmountResults(), UnorderedElementsAre());
  EXPECT_THAT(server_.getUnusedBindUnmountResults(), UnorderedElementsAre());

  // Re-register the unmount results for repo_x just to confirm that they are
  // not re-used on shutdown.
  server_.setFuseUnmountResult("/mnt/repo_x").setValue();
  server_.setBindUnmountResult("/mnt/repo_x/y").setValue();

  // Register results for the other unmount operations that should occur.
  server_.setFuseUnmountResult("/mnt/abc").setValue();
  server_.setBindUnmountResult("/mnt/abc/foo/buck-out").setValue();
  server_.setBindUnmountResult("/mnt/abc/xyz/test/buck-out").setValue();
  server_.setFuseUnmountResult("/mnt/xyz").setValue();
  server_.setBindUnmountResult("/mnt/xyz/buck-out").setValue();
  server_.setFuseUnmountResult("/data/users/johndoe/myrepo").setValue();

  // Shut down the privhelper.  It should unmount the registered mount points.
  cleanup();
  EXPECT_THAT(
      server_.getUnusedFuseUnmountResults(),
      UnorderedElementsAre("/mnt/repo_x"));
  EXPECT_THAT(
      server_.getUnusedBindUnmountResults(),
      UnorderedElementsAre("/mnt/repo_x/y"));
}

TEST_F(PrivHelperTest, detachEventBase) {
  // Perform one call using the current EventBase
  TemporaryFile tempFile;
  auto filePromise = server_.setFuseMountResult("/foo/bar");
  auto result = client_->fuseMount("/foo/bar");
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

    filePromise = server_.setFuseMountResult("/new/event/base");
    server_.setFuseUnmountResult("/new/event/base").setValue();
    result = client_->fuseMount("/new/event/base");
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
        .thenValue([&success](folly::File&&) { success = true; })
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
  auto unmountPromise = server_.setFuseUnmountResult("/foo/bar");
  auto unmountResult = client_->fuseUnmount("/foo/bar");
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

/*
 * A test that actually forks a separate privhelper process and verifies it
 * cleans up successfully.
 *
 * This is different than most of the tests above that simply run the privhelper
 * server as a separate thread in the same process.
 */
TEST(PrivHelper, ForkedServerShutdownTest) {
  TemporaryDirectory tmpDir;
  PrivHelperTestServer server;

  auto fooDir = tmpDir.path() / "foo";
  create_directory(fooDir);
  auto foo = fooDir.string();

  // Note we do not create this directory explicitly because we want to verify
  // that privilegedBindMount takes care of this for us.
  auto mountedBuckOut = tmpDir.path() / "foo" / "buck-out";

  auto barDir = tmpDir.path() / "bar";
  create_directory(barDir);
  auto bar = barDir.string();

  auto otherDir = (tmpDir.path() / "other");
  create_directory(barDir);
  auto other = otherDir.string();

  {
    auto privHelper = startPrivHelper(&server, UserInfo::lookup());
    EventBaseThread evbt;
    evbt.getEventBase()->runInEventBaseThreadAndWait(
        [&] { privHelper->attachEventBase(evbt.getEventBase()); });

    // Create a few mount points
    privHelper->fuseMount(foo).get(50ms);
    privHelper->fuseMount(bar).get(50ms);
    EXPECT_TRUE(server.isMounted(foo));
    EXPECT_TRUE(server.isMounted(bar));
    EXPECT_FALSE(server.isMounted(other));

    // Create a bind mount.
    EXPECT_FALSE(boost::filesystem::exists(mountedBuckOut));
    TemporaryDirectory realBuckOut;
    privHelper->bindMount(realBuckOut.path().c_str(), mountedBuckOut.c_str())
        .get(50ms);
    EXPECT_TRUE(server.isBindMounted(mountedBuckOut.c_str()));
    EXPECT_TRUE(boost::filesystem::exists(mountedBuckOut))
        << "privilegedBindMount() should create the bind mount directory for "
           "the caller.";

    // The privhelper will exit at the end of this scope
  }

  // Make sure things get umounted when the privhelper quits
  EXPECT_FALSE(server.isMounted(foo));
  EXPECT_FALSE(server.isBindMounted(mountedBuckOut.string()));
  EXPECT_FALSE(server.isMounted(bar));
  EXPECT_FALSE(server.isMounted(other));
}

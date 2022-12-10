/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <chrono>
#include <unordered_map>

#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/fuse/privhelper/PrivHelperImpl.h"
#include "eden/fs/fuse/privhelper/test/PrivHelperTestServer.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/UserInfo.h"

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

  folly::File fuseMount(const char* mountPath, bool /*readOnly*/) override {
    auto future = getResultFuture(data_.wlock()->fuseMountResults, mountPath);
    return std::move(future).get(1s);
  }

  void unmount(const char* mountPath) override {
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
        std::thread([this, conn = std::move(serverConn)]() mutable noexcept {
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
  auto mountPoint = makeTempDir("bar");
  auto path = mountPoint.path().string();

  // Prepare a promise to use as the result for trying to mount mountPoint
  auto filePromise = server_.setFuseMountResult(path);

  // Call fuseMount() this should return a future that is not ready yet,
  // since we have not fulfilled the promise.
  auto result = client_->fuseMount(path, false);
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

TEST_F(PrivHelperTest, fuseMountPermissions) {
  if (getuid() != 0) {
    auto path = folly::kIsApple ? "/var/root/bar" : "/root/bar";
    EXPECT_THROW_RE(
        client_->fuseMount(path, false).get(),
        std::exception,
        folly::to<std::string>(
            "User:",
            getuid(),
            " doesn't have write access to ",
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
      client_->fuseMount(path, false).get(),
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
  auto abcResult = client_->fuseMount(abcPath, false);
  auto defResult = client_->fuseMount(defPath, false);
  auto foobarResult = client_->fuseMount(barPath, false);
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
  // Leave the promise for somerepo/buck-out unfulfilled for now
  auto somerepoBuckOutUnmountPromise =
      server_.setBindUnmountResult(userPath + "/somerepo/buck-out");

  // Prepare some extra unmount promises that we don't expect to be used,
  // just to verify that cleanup happens as expected.
  server_.setFuseUnmountResult("/never/actually/mounted").setValue();
  server_.setBindUnmountResult("/bind/never/actually/mounted").setValue();

  // Mount everything
  client_->fuseMount(userPath + "/somerepo", false).get(1s);
  client_->bindMount("/bind/mount/source", userPath + "/somerepo/buck-out")
      .get(1s);

  client_->fuseMount(abcPath, false).get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/foo/buck-out").get(1s);
  client_->fuseMount(userPath + "/somerepo2", false).get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/bar/buck-out").get(1s);

  // Manually unmount /somerepo
  // This will finish even though somerepoBuckOutUnmountPromise is still
  // outstanding because the privhelper and the OS don't care about relative
  // ordering of these two operations.
  auto unmountResult = client_->fuseUnmount(userPath + "/somerepo");
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

TEST_F(PrivHelperTest, takeoverShutdown) {
  auto abcMountPoint = makeTempDir("abc");
  auto abcPath = abcMountPoint.path().string();
  TemporaryFile tempFile;

  boost::filesystem::create_directory(abcMountPoint.path() / "foo");
  boost::filesystem::create_directory(abcMountPoint.path() / "bar");

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
  client_->fuseMount(abcPath, false).get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/foo/buck-out").get(1s);
  client_->bindMount("/bind/mount/source", abcPath + "/bar/buck-out").get(1s);
  client_->fuseMount(userPath + "/somerepo", false).get(1s);
  client_->fuseMount(userPath + "/somerepo2", false).get(1s);
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
  server_.setFuseMountResult(xyzPath).setValue(File(tempFile.fd(), false));
  server_.setBindMountResult(xyzPath + "/buck-out").setValue();
  client_->fuseMount(xyzPath, false).get(1s);
  client_->bindMount("/bind/mount/source", xyzPath + "/buck-out").get(1s);

  // Manually unmount /mnt/repo_x
  server_.setFuseUnmountResult(repoXPath).setValue();
  server_.setBindUnmountResult(repoXPath + "/y").setValue();
  client_->fuseUnmount(repoXPath).get(1s);
  EXPECT_THAT(server_.getUnusedFuseUnmountResults(), UnorderedElementsAre());
  EXPECT_THAT(server_.getUnusedBindUnmountResults(), UnorderedElementsAre());

  // Re-register the unmount results for repo_x just to confirm that they are
  // not re-used on shutdown.
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
  auto result = client_->fuseMount(barPath, false);
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
    result = client_->fuseMount(newPath, false);
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
  auto unmountPromise = server_.setFuseUnmountResult(barPath);
  auto unmountResult = client_->fuseUnmount(barPath);
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

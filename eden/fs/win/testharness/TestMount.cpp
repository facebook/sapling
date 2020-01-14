/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "TestMount.h"

#include <folly/executors/ManualExecutor.h>
#include <folly/experimental/TestUtil.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <gtest/gtest.h>
#include <sys/stat.h>
#include <sys/types.h>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"
#include "eden/fs/win/mount/CurrentState.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/testharness/TestFsChannel.h"
#include "eden/fs/win/utils/FileUtils.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/Stub.h"
#include "eden/fs/win/utils/UserInfo.h"

using folly::Future;
using folly::makeFuture;
using folly::Unit;
using namespace std::chrono_literals;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::string;

namespace facebook {
namespace eden {

TestMount::TestMount()
    : privHelper_{make_shared<PrivHelper>()},
      serverExecutor_{make_shared<folly::ManualExecutor>()},
      testDir_{std::filesystem::temp_directory_path() /
               Guid::generate().toWString()} {
  // Initialize the temporary directory.
  // This sets both testDir_, config_, localStore_, and backingStore_
  initTestDirectory();
  UserInfo userInfo;

  serverState_ = {make_shared<ServerState>(
      userInfo,
      privHelper_,
      make_shared<UnboundedQueueExecutor>(serverExecutor_),
      clock_,
      make_shared<ProcessNameCache>(),
      make_shared<NullStructuredLogger>(),
      make_shared<EdenConfig>(
          /*userName=*/folly::StringPiece{userInfo.getUsername()},
          /*userID=*/uid_t{},
          /*userHomePath=*/AbsolutePath{testDir_.generic_string()},
          /*userConfigPath=*/
          AbsolutePath{(testDir_ / L".edenrc").generic_string()},
          /*systemConfigDir=*/AbsolutePath{testDir_.generic_string()},
          /*systemConfigPath=*/
          AbsolutePath{
              (testDir_ / L"edenfs.rc").generic_string(),
          }),
      /*enableFaultInjection=*/true)};
}

TestMount::TestMount(FakeTreeBuilder& rootBuilder, bool startReady)
    : TestMount() {
  initialize(rootBuilder, startReady);
}

TestMount::TestMount(FakeTreeBuilder&& rootBuilder)
    : TestMount(rootBuilder, /*startReady=*/true) {}

TestMount::TestMount(
    Hash initialCommitHash,
    FakeTreeBuilder& rootBuilder,
    bool startReady)
    : TestMount() {
  initialize(initialCommitHash, rootBuilder, startReady);
}

TestMount::~TestMount() {
  // The ObjectStore's futures can have a strong reference to an Inode which
  // has a reference to its parent, all the way to the root, which in effect
  // keeps the EdenMount alive, causing the test to leak.
  // Manually release the futures in FakeBackingStore.
  backingStore_->discardOutstandingRequests();

  // Make sure the server executor has nothing left to run.
  serverExecutor_->drain();

  CHECK_EQ(0, serverExecutor_->clear());
}

void TestMount::initialize(
    Hash initialCommitHash,
    std::chrono::system_clock::time_point lastCheckoutTime) {
  // Set the initial commit ID
  setInitialCommit(initialCommitHash);

  // Create edenMount_
  createMount();
  // edenMount_->initialize().getVia(serverExecutor_.get());
  // edenMount_->setLastCheckoutTime(lastCheckoutTime);
}

void TestMount::initialize(Hash commitHash, Hash rootTreeHash) {
  // Set the initial commit ID
  setInitialCommit(commitHash, rootTreeHash);

  // Create edenMount_
  createMount();
}

void TestMount::initialize(
    Hash initialCommitHash,
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  // Finalize rootBuilder and get the root Tree
  rootBuilder.finalize(backingStore_, startReady);
  auto rootTree = rootBuilder.getRoot();
  // We have to make sure the root tree is ready.  The EdenMount constructor
  // blocks until it is available, so we will hang below if it isn't ready.
  rootTree->setReady();

  // Set the commit to tree mapping, and record the current commit hash
  setInitialCommit(initialCommitHash, rootTree->get().getHash());

  // Create edenMount_
  createMount();
}

void TestMount::createMount() {
  shared_ptr<ObjectStore> objectStore = ObjectStore::create(
      localStore_,
      backingStore_,
      stats_,
      &folly::QueuedImmediateExecutor::instance());
  auto journal = std::make_unique<Journal>(stats_);
  edenMount_ = EdenMount::create(
      std::move(config_),
      std::move(objectStore),
      serverState_,
      std::move(journal));

  edenMount_->initialize(std::move(std::make_unique<TestFsChannel>()));

  winStore_ = make_unique<WinStore>(*(edenMount_.get()));

  edenMount_->start();
}

// void TestMount::registerFakeFuse(std::shared_ptr<FakeFuse> fuse) {
//  privHelper_->registerMount(edenMount_->getPath(), std::move(fuse));
//}

Hash TestMount::nextCommitHash() {
  auto number = commitNumber_.fetch_add(1);
  return makeTestHash(folly::to<string>(number));
}

void TestMount::initialize(FakeTreeBuilder& rootBuilder, bool startReady) {
  initialize(nextCommitHash(), rootBuilder, startReady);
}

void TestMount::initTestDirectory() {
  std::wcout << L"Running initTestDirectory in : " << testDir_;
  // Create the temporary directory
  std::filesystem::create_directories(testDir_);

  auto clientDirectory = testDir_ / L"eden";
  std::filesystem::create_directories(clientDirectory);
  std::filesystem::create_directories(clientDirectory / L"local");
  mountPath_ = testDir_ / L"mount";
  std::filesystem::create_directory(mountPath_);

  // Create the CheckoutConfig using our newly-populated client directory
  config_ = make_unique<CheckoutConfig>(
      AbsolutePathPiece{mountPath_.generic_string()},
      AbsolutePathPiece{clientDirectory.generic_string()});

  // Create localStore_ and backingStore_
  localStore_ = make_shared<MemoryLocalStore>();
  backingStore_ = make_shared<FakeBackingStore>(localStore_);

  stats_ = make_shared<EdenStats>();
}

void TestMount::remount() {
  // Create a new copy of the CheckoutConfig
  auto config = make_unique<CheckoutConfig>(*edenMount_->getConfig());
  // Create a new ObjectStore pointing to our local store and backing store
  auto objectStore = ObjectStore::create(
      localStore_,
      backingStore_,
      stats_,
      &folly::QueuedImmediateExecutor::instance());

  auto journal = std::make_unique<Journal>(stats_);

  // Reset the edenMount_ pointer.  This will destroy the old EdenMount
  // assuming that no-one else still has any references to it.
  //
  // We do this explicitly so that the old edenMount_ is destroyed before we
  // create the new one below.
  std::weak_ptr<EdenMount> weakMount = edenMount_;
  edenMount_.reset();
  EXPECT_EQ(0, weakMount.lock().use_count())
      << "All references to EdenMount should be released before calling "
         "remount()";

  // Create a new EdenMount object.
  edenMount_ = EdenMount::create(
      std::move(config),
      std::move(objectStore),
      serverState_,
      std::move(journal));
  edenMount_->initialize(std::move(std::make_unique<TestFsChannel>()));
  winStore_ = make_unique<WinStore>(*(edenMount_.get()));
  edenMount_->start();
}

void TestMount::resetCommit(FakeTreeBuilder& builder, bool setReady) {
  resetCommit(nextCommitHash(), builder, setReady);
}

void TestMount::resetCommit(
    Hash commitHash,
    FakeTreeBuilder& builder,
    bool setReady) {
  auto* rootTree = builder.finalize(backingStore_, setReady);
  auto* storedCommit =
      backingStore_->putCommit(commitHash, rootTree->get().getHash());
  storedCommit->setReady();

  // The root tree needs to be made ready too, even if setReady is false.
  // resetCommit() won't return until until the root tree can be loaded.
  if (!setReady) {
    rootTree->setReady();
  }

  edenMount_->resetParent(commitHash);
}

size_t TestMount::drainServerExecutor() {
  return serverExecutor_->drain();
}

void TestMount::setInitialCommit(Hash commitHash) {
  // Write the commit hash to the snapshot file
  auto snapshotPath = config_->getSnapshotPath();
  auto data = commitHash.toString() + "\n";
  writeFileAtomic(snapshotPath.c_str(), data);
}

void TestMount::setInitialCommit(Hash commitHash, Hash rootTreeHash) {
  // Record the commit hash to root tree hash mapping in the BackingStore
  auto* storedCommit = backingStore_->putCommit(commitHash, rootTreeHash);
  storedCommit->setReady();

  // Call setInitialCommit(hash) to write the snapshot file
  setInitialCommit(commitHash);
}

void TestMount::createEntry(
    const WinRelativePathW& path,
    bool isDirectory,
    folly::StringPiece hash) {
  FileMetadata metadata = {};

  if (!winStore_->getFileMetadata(std::wstring_view{path.c_str()}, metadata)) {
    throw std::logic_error(
        folly::sformat("File not found in the tree {}", path.generic_string()));
  }
  EXPECT_EQ(isDirectory, metadata.isDirectory);
  getMount()->getCurrentState()->entryCreated(path.c_str(), metadata);
}

void TestMount::loadEntry(const WinRelativePathW& path) {
  auto blob = winStore_->getBlob(std::wstring_view{path.c_str()});
  if (!blob) {
    throw std::logic_error(
        folly::sformat("Blob not found for : {}", path.generic_string()));
  }
  auto absolutePath = mountPath_ / path;
  create_directories(absolutePath.parent_path());
  writeFile(
      absolutePath.c_str(), blob->getContents().cloneAsValue().coalesce());
  getMount()->getCurrentState()->entryLoaded(path.c_str());
}

void TestMount::createFile(const WinRelativePathW& path, const char* data) {
  auto absolutePath = mountPath_ / path;

  create_directories(absolutePath.parent_path());
  writeFile(absolutePath.c_str(), data);
  getMount()->getCurrentState()->fileCreated(
      path.c_str(), /*isDirectory=*/false);
}

void TestMount::createDirectory(const WinRelativePathW& path) {
  auto absolutePath = mountPath_ / path;

  create_directories(absolutePath);
  getMount()->getCurrentState()->fileCreated(
      path.c_str(), /*isDirectory=*/true);
}

void TestMount::modifyFile(const WinRelativePathW& path, const char* data) {
  auto absolutePath = mountPath_ / path;

  writeFile(absolutePath.c_str(), folly::Range{data});
  getMount()->getCurrentState()->fileModified(
      path.c_str(), /* isDirectory=*/false);
}

void TestMount::removeFile(const WinRelativePathW& path) {
  auto absolutePath = mountPath_ / path;

  std::filesystem::remove(absolutePath);
  getMount()->getCurrentState()->fileRemoved(
      path.c_str(), /*isDirectory=*/false);
}

void TestMount::removeDirectory(const WinRelativePathW& path) {
  auto absolutePath = mountPath_ / path;

  std::filesystem::remove(absolutePath);
  getMount()->getCurrentState()->fileRemoved(
      path.c_str(), /*isDirectory=*/true);
}

void TestMount::renameFile(
    const WinRelativePathW& oldpath,
    const WinRelativePathW& newpath,
    bool isDirectory) {
  if (!isDirectory) {
    // We only delete files here and skip the directories to maintain the
    // directory structure. This helps with few tests.
    auto absoluteOldPath = mountPath_ / oldpath;
    auto absoluteNewPath = mountPath_ / newpath;

    std::filesystem::rename(absoluteOldPath, absoluteNewPath);
  }
  getMount()->getCurrentState()->fileRenamed(
      oldpath.c_str(), newpath.c_str(), isDirectory);
}

std::shared_ptr<const Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

} // namespace eden
} // namespace facebook

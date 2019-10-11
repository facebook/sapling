/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "TestMount.h"

#include <folly/FileUtil.h>
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
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/ProcessNameCache.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using folly::ByteRange;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
using namespace std::chrono_literals;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::string;
using std::unique_ptr;
using std::vector;

DEFINE_int32(
    num_eden_test_threads,
    2,
    "the number of eden CPU worker threads to create during unit tests");

namespace {
constexpr size_t kBlobCacheMaximumSize = 1000; // bytes
constexpr size_t kBlobCacheMinimumEntries = 0;
} // namespace

namespace facebook {
namespace eden {

bool TestMountFile::operator==(const TestMountFile& other) const {
  return path == other.path && contents == other.contents && rwx == other.rwx &&
      type == other.type;
}

TestMount::TestMount()
    : blobCache_{BlobCache::create(
          kBlobCacheMaximumSize,
          kBlobCacheMinimumEntries)},
      privHelper_{make_shared<FakePrivHelper>()},
      serverExecutor_{make_shared<folly::ManualExecutor>()} {
  // Initialize the temporary directory.
  // This sets both testDir_, config_, localStore_, and backingStore_
  initTestDirectory();

  serverState_ = {make_shared<ServerState>(
      UserInfo::lookup(),
      privHelper_,
      make_shared<UnboundedQueueExecutor>(serverExecutor_),
      clock_,
      make_shared<ProcessNameCache>(),
      make_shared<EdenConfig>(
          /*userName=*/folly::StringPiece{"bob"},
          /*userID=*/uid_t{},
          /*userHomePath=*/AbsolutePath{testDir_->path().string()},
          /*userConfigPath=*/
          AbsolutePath{testDir_->path().string() + ".edenrc"},
          /*systemConfigDir=*/AbsolutePath{testDir_->path().string()},
          /*systemConfigPath=*/
          AbsolutePath{testDir_->path().string() + "edenfs.rc"}))};
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
  edenMount_->initialize().getVia(serverExecutor_.get());
  edenMount_->setLastCheckoutTime(lastCheckoutTime);
}

void TestMount::initialize(Hash commitHash, Hash rootTreeHash) {
  // Set the initial commit ID
  setInitialCommit(commitHash, rootTreeHash);

  // Create edenMount_
  createMount();
  edenMount_->initialize().getVia(serverExecutor_.get());
}

void TestMount::initialize(
    Hash initialCommitHash,
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  createMountWithoutInitializing(
      initialCommitHash, rootBuilder, /*startReady=*/startReady);
  edenMount_->initialize().getVia(serverExecutor_.get());
}

void TestMount::createMountWithoutInitializing(
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
  shared_ptr<ObjectStore> objectStore =
      ObjectStore::create(localStore_, backingStore_, stats_);
  auto journal = std::make_unique<Journal>(stats_);
  edenMount_ = EdenMount::create(
      std::move(config_),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal));
}

void TestMount::registerFakeFuse(std::shared_ptr<FakeFuse> fuse) {
  privHelper_->registerMount(edenMount_->getPath(), std::move(fuse));
}

Hash TestMount::nextCommitHash() {
  auto number = commitNumber_.fetch_add(1);
  return makeTestHash(folly::to<string>(number));
}

void TestMount::initialize(FakeTreeBuilder& rootBuilder, bool startReady) {
  initialize(nextCommitHash(), rootBuilder, startReady);
}

void TestMount::createMountWithoutInitializing(
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  createMountWithoutInitializing(nextCommitHash(), rootBuilder, startReady);
}

void TestMount::initTestDirectory() {
  // Create the temporary directory
  testDir_ = makeTempDir();

  // Make the mount point and the eden client storage directories
  // inside the test directory.
  auto makedir = [](AbsolutePathPiece path) {
    ::mkdir(path.stringPiece().str().c_str(), 0755);
  };
  auto testDirPath = AbsolutePath{testDir_->path().string()};
  auto clientDirectory = testDirPath + "eden"_pc;
  makedir(clientDirectory);
  makedir(clientDirectory + "local"_pc);
  auto mountPath = testDirPath + "mount"_pc;
  makedir(mountPath);

  // Create the CheckoutConfig using our newly-populated client directory
  config_ = make_unique<CheckoutConfig>(mountPath, clientDirectory);

  // Create localStore_ and backingStore_
  localStore_ = make_shared<MemoryLocalStore>();
  backingStore_ = make_shared<FakeBackingStore>(localStore_);

  stats_ = make_shared<EdenStats>();
}

Dispatcher* TestMount::getDispatcher() const {
  return edenMount_->getDispatcher();
}

void TestMount::startFuseAndWait(std::shared_ptr<FakeFuse> fuse) {
  constexpr auto kTimeout = 10s;
  CHECK(edenMount_) << "Call initialize() before calling " << __func__;
  registerFakeFuse(fuse);
  auto startFuseFuture = edenMount_->startFuse();
  fuse->sendInitRequest();
  fuse->recvResponse();
  drainServerExecutor();
  std::move(startFuseFuture).get(kTimeout);
}

void TestMount::remount() {
  // Create a new copy of the CheckoutConfig
  auto config = make_unique<CheckoutConfig>(*edenMount_->getConfig());
  // Create a new ObjectStore pointing to our local store and backing store
  auto objectStore = ObjectStore::create(localStore_, backingStore_, stats_);

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
      blobCache_,
      serverState_,
      std::move(journal));
  edenMount_->initialize().getVia(serverExecutor_.get());
}

void TestMount::remountGracefully() {
  // Create a new copy of the CheckoutConfig
  auto config = make_unique<CheckoutConfig>(*edenMount_->getConfig());
  // Create a new ObjectStore pointing to our local store and backing store
  auto objectStore = ObjectStore::create(localStore_, backingStore_, stats_);

  auto journal = std::make_unique<Journal>(stats_);

  auto takeoverData =
      edenMount_->shutdown(/*doTakeover=*/true, /*allowFuseNotStarted=*/true)
          .get();

  // Reset the edenMount_ pointer.  This will destroy the old EdenMount
  // assuming that no-one else still has any references to it.
  //
  // We do this explicitly so that the old edenMount_ is destroyed before we
  // create the new one below.
  std::weak_ptr<EdenMount> weakMount = edenMount_;
  edenMount_.reset();
  EXPECT_EQ(0, weakMount.lock().use_count())
      << "All references to EdenMount should be released before calling "
         "remountGracefully()";

  XLOG(DBG1) << "number of unloaded inodes transferred on graceful remount: "
             << takeoverData.unloadedInodes.size();

  // Create a new EdenMount object.
  edenMount_ = EdenMount::create(
      std::move(config),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal));
  edenMount_->initialize(takeoverData).getVia(serverExecutor_.get());
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

bool TestMount::hasOverlayData(InodeNumber ino) const {
  return edenMount_->getOverlay()->hasOverlayData(ino);
}

bool TestMount::hasMetadata(InodeNumber ino) const {
  return edenMount_->getInodeMetadataTable()->getOptional(ino).has_value();
}

size_t TestMount::drainServerExecutor() {
  return serverExecutor_->drain();
}

void TestMount::setInitialCommit(Hash commitHash) {
  // Write the commit hash to the snapshot file
  auto snapshotPath = config_->getSnapshotPath();
  folly::writeFileAtomic(
      snapshotPath.stringPiece(), commitHash.toString() + "\n");
}

void TestMount::setInitialCommit(Hash commitHash, Hash rootTreeHash) {
  // Record the commit hash to root tree hash mapping in the BackingStore
  auto* storedCommit = backingStore_->putCommit(commitHash, rootTreeHash);
  storedCommit->setReady();

  // Call setInitialCommit(hash) to write the snapshot file
  setInitialCommit(commitHash);
}

void TestMount::addFile(folly::StringPiece path, folly::StringPiece contents) {
  RelativePathPiece relativePath(path);
  const auto treeInode = getTreeInode(relativePath.dirname());
  auto createResult =
      treeInode->create(relativePath.basename(), /*mode*/ 0644, /*flags*/ 0)
          .get();
  createResult.inode->write(contents, /*off*/ 0).get(0ms);
  createResult.inode->fsync(/*datasync*/ true);
}

void TestMount::addSymlink(
    folly::StringPiece path,
    folly::StringPiece pointsTo) {
  const RelativePathPiece relativePath{path};
  const auto parent = getTreeInode(relativePath.dirname());
  (void)parent->symlink(relativePath.basename(), pointsTo).get();
}

void TestMount::overwriteFile(folly::StringPiece path, std::string contents) {
  auto file = getFileInode(path);

  fuse_setattr_in attr;
  attr.valid = FATTR_SIZE;
  attr.size = 0;
  (void)file->setattr(attr).get(0ms);

  off_t offset = 0;
  file->write(contents, offset).get(0ms);
  file->fsync(/*datasync*/ true);
}

void TestMount::move(folly::StringPiece src, folly::StringPiece dest) {
  RelativePathPiece srcPath{src};
  RelativePathPiece destPath{dest};
  auto future = getTreeInode(srcPath.dirname())
                    ->rename(
                        srcPath.basename(),
                        getTreeInode(destPath.dirname()),
                        destPath.basename());
  std::move(future).get();
}

std::string TestMount::readFile(folly::StringPiece path) {
  return getFileInode(path)->readAll(CacheHint::LikelyNeededAgain).get();
}

bool TestMount::hasFileAt(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  mode_t mode;
  try {
    auto child = edenMount_->getInode(relativePath).get();
    mode = child->getattr().get().st.st_mode;
  } catch (const std::system_error& e) {
    if (e.code().value() == ENOENT) {
      return false;
    } else {
      throw;
    }
  }

  return S_ISREG(mode);
}

void TestMount::mkdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  mode_t mode = 0755;
  (void)treeInode->mkdir(relativePath.basename(), mode).get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  treeInode->unlink(relativePath.basename()).get();
}

void TestMount::rmdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  treeInode->rmdir(relativePath.basename()).get();
}

void TestMount::chmod(folly::StringPiece path, mode_t permissions) {
  auto inode = getInode(RelativePathPiece{path});

  fuse_setattr_in desiredAttr = {};
  desiredAttr.mode = permissions;
  desiredAttr.valid = FATTR_MODE;
  inode->setattr(desiredAttr).get();
}

InodePtr TestMount::getInode(RelativePathPiece path) const {
  // Call future.get() with a timeout.  Generally in tests we expect the future
  // to be immediately ready.  We want to make sure the test does not hang
  // forever if something goes wrong.
  return edenMount_->getInode(path).get(std::chrono::milliseconds(1));
}

InodePtr TestMount::getInode(folly::StringPiece path) const {
  return getInode(RelativePathPiece{path});
}

TreeInodePtr TestMount::getTreeInode(RelativePathPiece path) const {
  return getInode(path).asTreePtr();
}

TreeInodePtr TestMount::getTreeInode(folly::StringPiece path) const {
  return getTreeInode(RelativePathPiece{path});
}

FileInodePtr TestMount::getFileInode(RelativePathPiece path) const {
  return getInode(path).asFilePtr();
}

FileInodePtr TestMount::getFileInode(folly::StringPiece path) const {
  return getFileInode(RelativePathPiece{path});
}

void TestMount::loadAllInodes() {
  loadAllInodesFuture().get();
}

Future<Unit> TestMount::loadAllInodesFuture() {
  return loadAllInodesFuture(edenMount_->getRootInode());
}

void TestMount::loadAllInodes(const TreeInodePtr& treeInode) {
  loadAllInodesFuture(treeInode).get();
}

Future<Unit> TestMount::loadAllInodesFuture(const TreeInodePtr& treeInode) {
  // Build a list of child names to load.
  // (If necessary we could make a more efficient version of this that starts
  // all the child loads while holding the lock.  However, we don't really care
  // about efficiency for test code, and this is much simpler.)
  std::vector<PathComponent> childNames;
  {
    auto contents = treeInode->getContents().rlock();
    for (const auto& entry : contents->entries) {
      childNames.emplace_back(entry.first);
    }
  }

  // Now start all the loads.
  std::vector<Future<Unit>> childFutures;
  for (const auto& name : childNames) {
    auto childFuture =
        treeInode->getOrLoadChild(name).thenValue([](InodePtr child) {
          TreeInodePtr childTree = child.asTreePtrOrNull();
          if (childTree) {
            return loadAllInodesFuture(childTree);
          }
          return makeFuture();
        });
    childFutures.emplace_back(std::move(childFuture));
  }
  return folly::collect(childFutures).unit();
}

std::shared_ptr<const Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

} // namespace eden
} // namespace facebook

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestMount.h"

#include <folly/FileUtil.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <folly/testing/TestUtil.h>
#include <gflags/gflags.h>
#include <gtest/gtest.h>
#include <sys/stat.h>
#include <sys/types.h>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/Guid.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/common/utils/UserInfo.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/EdenDispatcherFactory.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/notifications/CommandNotifier.h"
#include "eden/fs/service/PrettyPrinters.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IScribeLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakeInodeAccessLogger.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestConfigSource.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/NotImplemented.h"

using folly::Unit;
using namespace std::chrono_literals;
using namespace std::string_literals;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::string;

DEFINE_int32(
    num_eden_test_threads,
    2,
    "the number of eden CPU worker threads to create during unit tests");

namespace facebook::eden {

namespace {
constexpr size_t kBlobCacheMaximumSize = 1000; // bytes
constexpr size_t kBlobCacheMinimumEntries = 0;
} // namespace

bool TestMountFile::operator==(const TestMountFile& other) const {
  return path == other.path && contents == other.contents && rwx == other.rwx &&
      type == other.type;
}

TestMount::TestMount(bool enableActivityBuffer, CaseSensitivity caseSensitivity)
    : privHelper_{make_shared<FakePrivHelper>()},
      serverExecutor_{make_shared<folly::ManualExecutor>()} {
  // Initialize the temporary directory.
  // This sets both testDir_, config_, and localStore_
  initTestDirectory(caseSensitivity);

  testConfigSource_ =
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig);

  edenConfig_ = make_shared<EdenConfig>(
      ConfigVariables{},
      /*userHomePath=*/canonicalPath(testDir_->path().string()),
      /*systemConfigDir=*/canonicalPath(testDir_->path().string()),
      EdenConfig::SourceVector{testConfigSource_});
  edenConfig_->enableActivityBuffer.setValue(
      enableActivityBuffer, ConfigSourceType::Default, true);

  // By default, we should not allocate an NFS server (including mountd and
  // rpcbindd) on an EventBase in unit tests.
  edenConfig_->enableNfsServer.setValue(false, ConfigSourceType::Default, true);

  auto reloadableConfig = std::make_shared<ReloadableConfig>(edenConfig_);

  // create blobCache
  blobCache_ = BlobCache::create(
      kBlobCacheMaximumSize,
      kBlobCacheMinimumEntries,
      reloadableConfig,
      makeRefPtr<EdenStats>());

  // Create treeCache
  treeCache_ = TreeCache::create(reloadableConfig, makeRefPtr<EdenStats>());

  serverState_ = make_shared<ServerState>(
      UserInfo::lookup(),
      makeRefPtr<EdenStats>(),
      SessionInfo{},
      privHelper_,
      make_shared<UnboundedQueueExecutor>(serverExecutor_),
      serverExecutor_,
      clock_,
      make_shared<ProcessInfoCache>(),
      make_shared<NullStructuredLogger>(),
      make_shared<NullStructuredLogger>(),
      make_shared<NullScribeLogger>(),
      reloadableConfig,
      *edenConfig_,
      nullptr,
      make_shared<CommandNotifier>(reloadableConfig),
      /*enableFaultInjection=*/true,
      make_shared<FakeInodeAccessLogger>());

  backingStore_ = make_shared<FakeBackingStore>(
      BackingStore::LocalStoreCachingPolicy::NoCaching, serverState_);
}

TestMount::TestMount(
    FakeTreeBuilder& rootBuilder,
    bool startReady,
    bool enableActivityBuffer,
    CaseSensitivity caseSensitivity)
    : TestMount(enableActivityBuffer, caseSensitivity) {
  // Create treeCache
  edenConfig_ = EdenConfig::createTestEdenConfig();

  auto edenConfig = std::make_shared<ReloadableConfig>(
      edenConfig_, ConfigReloadBehavior::NoReload);
  treeCache_ = TreeCache::create(edenConfig, makeRefPtr<EdenStats>());
  initialize(rootBuilder, startReady);
}

TestMount::TestMount(
    FakeTreeBuilder&& rootBuilder,
    bool enableActivityBuffer,
    CaseSensitivity caseSensitivity)
    : TestMount(
          rootBuilder,
          /*startReady=*/true,
          enableActivityBuffer,
          caseSensitivity) {
  XCHECK_NE(edenConfig_, nullptr);
  XCHECK_NE(treeCache_, nullptr);
}

TestMount::TestMount(
    const RootId& initialCommitId,
    FakeTreeBuilder& rootBuilder,
    bool startReady,
    bool enableActivityBuffer,
    CaseSensitivity caseSensitivity)
    : TestMount(enableActivityBuffer, caseSensitivity) {
  edenConfig_ = EdenConfig::createTestEdenConfig();

  // Create treeCache
  auto edenConfig = std::make_shared<ReloadableConfig>(
      edenConfig_, ConfigReloadBehavior::NoReload);
  treeCache_ = TreeCache::create(edenConfig, makeRefPtr<EdenStats>());
  initialize(initialCommitId, rootBuilder, startReady);
}

TestMount::TestMount(CaseSensitivity caseSensitivity)
    : TestMount(/*enableActivityBuffer=*/true, caseSensitivity) {}

TestMount::~TestMount() {
  // The ObjectStore's futures can have a strong reference to an Inode which
  // has a reference to its parent, all the way to the root, which in effect
  // keeps the EdenMount alive, causing the test to leak.
  // Manually release the futures in FakeBackingStore.
  backingStore_->discardOutstandingRequests();

  // Make sure the server executor has nothing left to run.
  serverExecutor_->drain();

  XCHECK_EQ(0ul, serverExecutor_->clear());
}

void TestMount::initialize(
    const RootId& initialCommitId,
    std::chrono::system_clock::time_point lastCheckoutTime) {
  // Set the initial commit ID
  setInitialCommit(initialCommitId);

  // Create edenMount_
  createMount();

  initializeEdenMount();
  edenMount_->setLastCheckoutTime(
      EdenTimestamp{folly::to<struct timespec>(lastCheckoutTime)});
}

void TestMount::initialize(const RootId& commitId, ObjectId rootTreeId) {
  // Set the initial commit ID
  setInitialCommit(commitId, rootTreeId);

  // Create edenMount_
  createMount();
  initializeEdenMount();
}

void TestMount::initialize(
    const RootId& initialCommitId,
    FakeTreeBuilder& rootBuilder,
    bool startReady,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions) {
  createMountWithoutInitializing(
      initialCommitId,
      rootBuilder,
      startReady,
      inodeCatalogType,
      inodeCatalogOptions);
  initializeEdenMount();
}

void TestMount::initializeEdenMount() {
  edenMount_->initialize()
      .semi()
      .via(serverExecutor_.get())
      .getVia(serverExecutor_.get());
  rootInode_ = edenMount_->getRootInodeUnchecked();
}

void TestMount::createMountWithoutInitializing(
    const RootId& initialCommitId,
    FakeTreeBuilder& rootBuilder,
    bool startReady,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions) {
  // Finalize rootBuilder and get the root Tree
  rootBuilder.finalize(backingStore_, startReady);
  auto rootTree = rootBuilder.getRoot();
  // We have to make sure the root tree is ready.  The EdenMount constructor
  // blocks until it is available, so we will hang below if it isn't ready.
  rootTree->setReady();

  // Set the commit to tree mapping, and record the current commit id
  setInitialCommit(initialCommitId, rootTree->get().getObjectId());

  // Create edenMount_
  createMount(inodeCatalogType, inodeCatalogOptions);
}

void TestMount::createMount(
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions) {
  shared_ptr<ObjectStore> objectStore = ObjectStore::create(
      backingStore_,
      localStore_,
      treeCache_,
      stats_.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<ReloadableConfig>(
          edenConfig_, ConfigReloadBehavior::NoReload),
      config_->getEnableWindowsSymlinks(),
      config_->getCaseSensitive());
  auto journal = std::make_unique<Journal>(stats_.copy());
  edenMount_ = EdenMount::create(
      std::move(config_),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal),
      stats_.copy(),
      inodeCatalogType,
      inodeCatalogOptions);
#ifndef _WIN32
  dispatcher_ = EdenDispatcherFactory::makeFuseDispatcher(edenMount_.get());
#endif
}

#ifndef _WIN32
void TestMount::registerFakeFuse(std::shared_ptr<FakeFuse> fuse) {
  privHelper_->registerMount(edenMount_->getPath(), std::move(fuse));
}
#endif

RootId TestMount::nextCommitId() {
  auto number = commitNumber_.fetch_add(1);
  return RootId{folly::to<string>(number)};
}

void TestMount::initialize(FakeTreeBuilder& rootBuilder, bool startReady) {
  initialize(nextCommitId(), rootBuilder, startReady);
}

void TestMount::initialize(
    FakeTreeBuilder& rootBuilder,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions) {
  initialize(
      nextCommitId(),
      rootBuilder,
      /* startReady */ true,
      inodeCatalogType,
      inodeCatalogOptions);
}

void TestMount::createMountWithoutInitializing(
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  createMountWithoutInitializing(nextCommitId(), rootBuilder, startReady);
}

void TestMount::initTestDirectory(CaseSensitivity caseSensitivity) {
  // Create the temporary directory
  testDir_ = std::make_unique<folly::test::TemporaryDirectory>(makeTempDir());

  // Make the mount point and the eden client storage directories
  // inside the test directory.
  auto tmpPath = testDir_->path().string();
  auto testDirPath = canonicalPath(tmpPath);
  auto clientDirectory = testDirPath + "eden"_pc;
  ensureDirectoryExists(clientDirectory + "local"_pc);
  auto mountPath = testDirPath + "mount"_pc;
  ensureDirectoryExists(mountPath);
  auto configPath = clientDirectory + "config.toml"_pc;
  auto configData =
      "[repository]\n"s
      "path = \"/test\"\n"
      "type = \"test\"\n"
      "case-sensitive = " +
      (caseSensitivity == CaseSensitivity::Sensitive ? "true" : "false") + "\n";
  writeFile(configPath, folly::ByteRange{configData}).value();

  // Create the CheckoutConfig using our newly-populated client directory
  config_ = CheckoutConfig::loadFromClientDirectory(mountPath, clientDirectory);

  // Create localStore_
  localStore_ = make_shared<MemoryLocalStore>(makeRefPtr<EdenStats>());

  stats_ = makeRefPtr<EdenStats>();
}

#ifndef _WIN32
FuseDispatcher* TestMount::getDispatcher() const {
  return dispatcher_.get();
}

void TestMount::startFuseAndWait(std::shared_ptr<FakeFuse> fuse) {
  constexpr auto kTimeout = 10s;
  XCHECK(edenMount_) << "Call initialize() before calling " << __func__;
  registerFakeFuse(fuse);
  auto startChannelFuture = edenMount_->startFsChannel(false);
  fuse->sendInitRequest();
  fuse->recvResponse();
  drainServerExecutor();
  std::move(startChannelFuture).get(kTimeout);
}
#endif

void TestMount::updateEdenConfig(
    const std::map<std::string, std::string>& values) {
  updateTestEdenConfig(
      testConfigSource_, serverState_->getReloadableConfig(), values);
}

void TestMount::remount() {
  // Create a new copy of the CheckoutConfig
  auto config = make_unique<CheckoutConfig>(*edenMount_->getCheckoutConfig());
  // Create a new ObjectStore pointing to our local store and backing store
  auto objectStore = ObjectStore::create(
      backingStore_,
      localStore_,
      treeCache_,
      stats_.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<ReloadableConfig>(
          edenConfig_, ConfigReloadBehavior::NoReload),
      config->getEnableWindowsSymlinks(),
      config->getCaseSensitive());

  auto journal = std::make_unique<Journal>(stats_.copy());

  // Reset the edenMount_ pointer.  This will destroy the old EdenMount
  // assuming that no-one else still has any references to it.
  //
  // We do this explicitly so that the old edenMount_ is destroyed before we
  // create the new one below.
  std::weak_ptr<EdenMount> weakMount = edenMount_;
  rootInode_.reset();
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
      std::move(journal),
      stats_.copy(),
      kDefaultInodeCatalogType,
      kDefaultInodeCatalogOptions);
  initializeEdenMount();
}

#ifndef _WIN32
void TestMount::remountGracefully() {
  // Create a new copy of the CheckoutConfig
  auto config = make_unique<CheckoutConfig>(*edenMount_->getCheckoutConfig());
  // Create a new ObjectStore pointing to our local store and backing store
  auto objectStore = ObjectStore::create(
      backingStore_,
      localStore_,
      treeCache_,
      stats_.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<ReloadableConfig>(
          edenConfig_, ConfigReloadBehavior::NoReload),
      config->getEnableWindowsSymlinks(),
      config->getCaseSensitive());

  auto journal = std::make_unique<Journal>(stats_.copy());

  rootInode_.reset();

  auto takeoverData =
      edenMount_->shutdown(/*doTakeover=*/true, /*allowFuseNotStarted=*/true)
          .get(1s);

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

  XLOGF(
      DBG1,
      "number of unloaded inodes transferred on graceful remount: {}",
      takeoverData.unloadedInodes()->size());

  // Create a new EdenMount object.
  edenMount_ = EdenMount::create(
      std::move(config),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal),
      stats_.copy(),
      kDefaultInodeCatalogType,
      kDefaultInodeCatalogOptions);
  edenMount_
      ->initialize(
          [](auto) {},
          takeoverData,
          std::optional<MountProtocol>(MountProtocol::FUSE))
      .semi()
      .via(serverExecutor_.get())
      .getVia(serverExecutor_.get());
  rootInode_ = edenMount_->getRootInodeUnchecked();
}
#endif

void TestMount::resetCommit(FakeTreeBuilder& builder, bool setReady) {
  resetCommit(nextCommitId(), builder, setReady);
}

void TestMount::resetCommit(
    const RootId& commitId,
    FakeTreeBuilder& builder,
    bool setReady) {
  auto* rootTree = builder.finalize(backingStore_, setReady);
  auto* storedCommit =
      backingStore_->putCommit(commitId, rootTree->get().getObjectId());
  storedCommit->setReady();

  // The root tree needs to be made ready too, even if setReady is false.
  // resetCommit() won't return until until the root tree can be loaded.
  if (!setReady) {
    rootTree->setReady();
  }

  edenMount_->resetParent(commitId);
}

bool TestMount::hasOverlayDir(InodeNumber ino) const {
  return edenMount_->getOverlay()->hasOverlayDir(ino);
}

bool TestMount::hasMetadata([[maybe_unused]] InodeNumber ino) const {
#ifndef _WIN32
  return edenMount_->getInodeMetadataTable()->getOptional(ino).has_value();
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

size_t TestMount::drainServerExecutor() {
  return serverExecutor_->drain();
}

void TestMount::setInitialCommit(const RootId& commitId) {
  // Write the commit id to the snapshot file
  config_->setCheckedOutCommit(commitId);
}

void TestMount::setInitialCommit(const RootId& commitId, ObjectId rootTreeId) {
  // Record the commit id to root tree id mapping in the BackingStore
  auto* storedCommit = backingStore_->putCommit(commitId, rootTreeId);
  storedCommit->setReady();

  // Call setInitialCommit(id) to write the snapshot file
  setInitialCommit(commitId);
}

void TestMount::addFile(folly::StringPiece path, folly::StringPiece contents) {
  RelativePathPiece relativePath(path);
  const auto treeInode = getTreeInode(relativePath.dirname());
  auto createResult = treeInode->mknod(
      relativePath.basename(),
      /*mode=*/S_IFREG | 0644,
      /*rdev=*/0,
      InvalidationRequired::No);
#ifdef _WIN32
  auto absolutePath =
      edenMount_->getCheckoutConfig()->getMountPath() + relativePath;
  // Make sure the directory exist.
  ensureDirectoryExists(absolutePath.dirname());
  // Create the file in the File System and also update the EdenMount. In the
  // real system with Projected FS, the creation of a file with send the
  // notification which will update the EdenMount.
  writeFile(absolutePath, contents).value();
#else
  createResult->write(contents, /*off*/ 0, ObjectFetchContext::getNullContext())
      .get(0ms);
  createResult->fsync(/*datasync*/ true);
#endif
}

#ifndef _WIN32
void TestMount::addSymlink(
    folly::StringPiece path,
    folly::StringPiece pointsTo) {
  const RelativePathPiece relativePath{path};
  const auto parent = getTreeInode(relativePath.dirname());
  parent->symlink(relativePath.basename(), pointsTo, InvalidationRequired::No)
      .get();
}
#endif

FileInodePtr TestMount::overwriteFile(
    folly::StringPiece path,
    folly::StringPiece contents) {
  RelativePathPiece relativePath(path);
  auto file = getFileInode(relativePath);

#ifdef _WIN32
  auto absolutePath =
      edenMount_->getCheckoutConfig()->getMountPath() + relativePath;
  // Make sure the directory exist.
  ensureDirectoryExists(absolutePath.dirname());
  // Write the file in the File System and also update the EdenMount. In the
  // real system with Projected FS, the closing of a modified file with send the
  // notification which will update the EdenMount.
  writeFile(absolutePath, contents).value();
  file->materialize();

  // Verify that what we wrote can be read back.
  auto newContents = readFile(path);
  EXPECT_EQ(newContents, contents);
#else
  DesiredMetadata desired;
  desired.size = 0;
  (void)file->setattr(desired, ObjectFetchContext::getNullContext()).get(0ms);

  FileOffset offset = 0;
  file->write(contents, offset, ObjectFetchContext::getNullContext()).get(0ms);
  file->fsync(/*datasync*/ true);
#endif

  return file;
}

void TestMount::move(folly::StringPiece src, folly::StringPiece dest) {
  RelativePathPiece srcPath{src};
  RelativePathPiece destPath{dest};

#ifdef _WIN32
  auto absoluteSrcPath =
      edenMount_->getCheckoutConfig()->getMountPath() + srcPath;
  auto absoluteDestPath =
      edenMount_->getCheckoutConfig()->getMountPath() + destPath;
  renameWithAbsolutePath(absoluteSrcPath, absoluteDestPath);
#endif

  auto future = getTreeInode(srcPath.dirname())
                    ->rename(
                        srcPath.basename(),
                        getTreeInode(destPath.dirname()),
                        destPath.basename(),
                        InvalidationRequired::No,
                        ObjectFetchContext::getNullContext())
                    .semi()
                    .via(getServerExecutor().get());
  drainServerExecutor();
  std::move(future).get(0ms);
}

std::string TestMount::readFile(folly::StringPiece path) {
  auto fut = getFileInode(path)
                 ->readAll(
                     ObjectFetchContext::getNullContext(),
                     CacheHint::LikelyNeededAgain)
                 .semi()
                 .via(getServerExecutor().get());
  drainServerExecutor();
  return std::move(fut).get(0ms);
}

bool TestMount::hasFileAt(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  try {
    auto fut =
        edenMount_
            ->getInodeSlow(relativePath, ObjectFetchContext::getNullContext())
            .semi()
            .via(getServerExecutor().get());
    drainServerExecutor();
    auto child = std::move(fut).get(0ms);
    return child->getType() == dtype_t::Regular;
  } catch (const std::system_error& e) {
    if (e.code().value() == ENOENT) {
      return false;
    } else {
      throw;
    }
  }
}

void TestMount::mkdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());

#ifdef _WIN32
  auto absolutePath =
      edenMount_->getCheckoutConfig()->getMountPath() + relativePath;
  ensureDirectoryExists(absolutePath.dirname());
#endif

  mode_t mode = 0755;
  (void)treeInode
      ->mkdir(relativePath.basename(), mode, InvalidationRequired::No)
      .get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());

#ifdef _WIN32
  auto absolutePath =
      edenMount_->getCheckoutConfig()->getMountPath() + relativePath;
  removeFileWithAbsolutePath(absolutePath);
#endif

  auto fut = treeInode
                 ->unlink(
                     relativePath.basename(),
                     InvalidationRequired::No,
                     ObjectFetchContext::getNullContext())
                 .semi()
                 .via(getServerExecutor().get());
  drainServerExecutor();
  std::move(fut).get(0ms);
}

void TestMount::rmdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());

#ifdef _WIN32
  auto absolutePath =
      edenMount_->getCheckoutConfig()->getMountPath() + relativePath;
  removeRecursively(absolutePath);
#endif

  auto fut = treeInode
                 ->rmdir(
                     relativePath.basename(),
                     InvalidationRequired::No,
                     ObjectFetchContext::getNullContext())
                 .semi()
                 .via(getServerExecutor().get());
  drainServerExecutor();
  std::move(fut).get(0ms);
}

#ifndef _WIN32
void TestMount::chmod(folly::StringPiece path, mode_t permissions) {
  auto inode = getInode(RelativePathPiece{path});

  DesiredMetadata desiredAttr;
  desiredAttr.mode = permissions;
  auto fut = inode->setattr(desiredAttr, ObjectFetchContext::getNullContext())
                 .semi()
                 .via(getServerExecutor().get());
  drainServerExecutor();
  std::move(fut).get(0ms);
}
#endif

InodePtr TestMount::getInode(RelativePathPiece path) const {
  // Call future.get() with a timeout.  Generally in tests we expect the future
  // to be immediately ready.  We want to make sure the test does not hang
  // forever if something goes wrong.
  auto fut =
      edenMount_->getInodeSlow(path, ObjectFetchContext::getNullContext())
          .semi()
          .via(getServerExecutor().get());
  getServerExecutor()->drain();
  return std::move(fut).get(std::chrono::milliseconds(100));
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

VirtualInode TestMount::getVirtualInode(RelativePathPiece path) const {
  auto fut =
      edenMount_
          ->getVirtualInode(
              RelativePathPiece{path}, ObjectFetchContext::getNullContext())
          .semi()
          .via(getServerExecutor().get());
  getServerExecutor()->drain();
  return std::move(fut).get(std::chrono::milliseconds(1));
}

VirtualInode TestMount::getVirtualInode(folly::StringPiece path) const {
  return getVirtualInode(RelativePathPiece{path});
}

void TestMount::loadAllInodes() {
  auto fut = loadAllInodesFuture().semi().via(getServerExecutor().get());
  drainServerExecutor();
  std::move(fut).get(std::chrono::milliseconds(1));
}

ImmediateFuture<Unit> TestMount::loadAllInodesFuture() {
  return loadAllInodesFuture(edenMount_->getRootInode());
}

void TestMount::loadAllInodes(const TreeInodePtr& treeInode) {
  loadAllInodesFuture(treeInode).get();
}

ImmediateFuture<Unit> TestMount::loadAllInodesFuture(
    const TreeInodePtr& treeInode) {
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
  std::vector<ImmediateFuture<Unit>> childFutures;
  for (const auto& name : childNames) {
    auto childFuture =
        treeInode->getOrLoadChild(name, ObjectFetchContext::getNullContext())
            .thenValue([](InodePtr child) -> ImmediateFuture<folly::Unit> {
              TreeInodePtr childTree = child.asTreePtrOrNull();
              if (childTree) {
                return loadAllInodesFuture(childTree);
              }
              return folly::unit;
            });
    childFutures.emplace_back(std::move(childFuture));
  }
  return collectAllSafe(std::move(childFutures)).unit();
}

std::shared_ptr<const Tree> TestMount::getRootTree() const {
  return edenMount_->getCheckedOutRootTree();
}

} // namespace facebook::eden

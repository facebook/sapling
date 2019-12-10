/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <sys/stat.h>
#include <optional>
#include <vector>
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/mount/EdenMount.h"
#include "eden/fs/win/utils/StringConv.h"

namespace folly {
class ManualExecutor;
} // namespace folly

namespace facebook {
namespace eden {
class CheckoutConfig;
class FakeBackingStore;
class PrivHelper;
class FakeTreeBuilder;
class LocalStore;
class EdenDispatcher;
class WinStore;

class TestMount {
 public:
  /**
   * Create a new uninitialized TestMount.
   *
   * The TestMount will not be fully initialized yet.  The caller must
   * populate the object store as desired, and then call initialize() to
   * create the underlying EdenMount object once the commit has been set up.
   */
  TestMount();

  /**
   * Create a new TestMount
   *
   * If startReady is true, all of the Tree and Blob objects created by the
   * rootBuilder will be made immediately ready in the FakeBackingStore.  If
   * startReady is false the objects will not be ready, and attempts to
   * retrieve them from the backing store will not complete until the caller
   * explicitly marks them ready.
   *
   * However, the root Tree object is always marked ready.  This is necessary
   * to create the EdenMount object.
   *
   * If an initialCommitHash is not explicitly specified, makeTestHash("1")
   * will be used.
   */
  explicit TestMount(FakeTreeBuilder& rootBuilder, bool startReady = true);
  explicit TestMount(FakeTreeBuilder&& rootBuilder);
  TestMount(
      Hash initialCommitHash,
      FakeTreeBuilder& rootBuilder,
      bool startReady = true);

  ~TestMount();

  /**
   * Initialize the mount.
   *
   * This should only be used if the TestMount was default-constructed.
   * The caller must have already defined the root commit.  The lastCheckoutTime
   * is read from the FakeClock.
   */
  void initialize(Hash initialCommitHash) {
    initialize(initialCommitHash, getClock().getTimePoint());
  }

  /**
   * Initialize the mount.
   *
   * This should only be used if the TestMount was default-constructed.
   * The caller must have already defined the root commit.
   */
  void initialize(
      Hash initialCommitHash,
      std::chrono::system_clock::time_point lastCheckoutTime);

  /**
   * Initialize the mount.
   *
   * This should only be used if the TestMount was default-constructed.
   * The caller must have already defined the root Tree in the object store.
   */
  void initialize(Hash initialCommitHash, Hash rootTreeHash);

  /**
   * Initialize the mount from the given root tree.
   *
   * This should only be used if the TestMount was default-constructed.
   *
   * If an initialCommitHash is not explicitly specified, makeTestHash("1")
   * will be used.
   */
  void initialize(
      Hash initialCommitHash,
      FakeTreeBuilder& rootBuilder,
      bool startReady = true);
  void initialize(FakeTreeBuilder& rootBuilder, bool startReady = true);

  /**
   * Get the CheckoutConfig object.
   *
   * The CheckoutConfig object provides methods to get the paths to the mount
   * point, the client directory, etc.
   */
  CheckoutConfig* getConfig() const {
    return config_.get();
  }

  /**
   * Callers can use this to populate the LocalStore before calling build().
   */
  const std::shared_ptr<LocalStore>& getLocalStore() const {
    return localStore_;
  }

  /**
   * Callers can use this to populate the BackingStore before calling build().
   */
  const std::shared_ptr<FakeBackingStore>& getBackingStore() const {
    return backingStore_;
  }

  /**
   * Access to the TestMount's FakeClock which is referenced by the underlying
   * EdenMount (and thus inodes).
   */
  FakeClock& getClock() {
    return *clock_;
  }

  /**
   * Re-create the EdenMount object, simulating a scenario where it was
   * unmounted and then remounted.
   *
   * Note that if the caller is holding references to the old EdenMount object
   * this will prevent it from being destroyed.  This may result in an error
   * trying to create the new EdenMount if the old mount object still exists
   * and is still holding a lock on the overlay or other data structures.
   */
  void remount();

  /**
   * Following are the list of operations that could be performed on the mount
   * through projected fs.
   */

  void createEntry(
      const WinRelativePathW& path,
      bool isDirectory,
      folly::StringPiece hash);

  void loadEntry(const WinRelativePathW& path);

  void createFile(const WinRelativePathW& path, const char* data);

  void createDirectory(const WinRelativePathW& path);

  void modifyFile(const WinRelativePathW& path, const char* data);

  void removeFile(const WinRelativePathW& path);

  void removeDirectory(const WinRelativePathW& path);

  void renameFile(
      const WinRelativePathW& oldpath,
      const WinRelativePathW& newpath,
      bool isDirectory);

  /** Convenience method for getting the Tree for the root of the mount. */
  std::shared_ptr<const Tree> getRootTree() const;

  std::shared_ptr<EdenMount>& getEdenMount() & noexcept {
    return edenMount_;
  }

  const std::shared_ptr<EdenMount>& getEdenMount() const& {
    return edenMount_;
  }

  const std::shared_ptr<ServerState>& getServerState() const {
    return serverState_;
  }

  /**
   * Get a hash to use for the next commit.
   *
   * This mostly just helps pick easily readable commit IDs that increment
   * over the course of a test.
   *
   * This returns "0000000000000000000000000000000000000001" on the first call,
   * "0000000000000000000000000000000000000002" on the second, etc.
   */
  Hash nextCommitHash();

  /**
   * Helper function to create a commit from a FakeTreeBuilder and call
   * EdenMount::resetCommit() with the result.
   */
  void resetCommit(FakeTreeBuilder& builder, bool setReady);
  void resetCommit(Hash commitHash, FakeTreeBuilder& builder, bool setReady);

  /**
   * Returns number of functions executed.
   */
  size_t drainServerExecutor();

  std::shared_ptr<folly::ManualExecutor> getServerExecutor() const {
    return serverExecutor_;
  }

  EdenMount* getMount() {
    return edenMount_.get();
  }

 private:
  void createMount();
  void initTestDirectory();
  void setInitialCommit(Hash commitHash);
  void setInitialCommit(Hash commitHash, Hash rootTreeHash);

  /**
   * The temporary directory for this TestMount.
   *
   * This must be stored as a member variable to ensure the temporary directory
   * lives for the duration of the test.
   *
   * We intentionally list it before the edenMount_ so it gets constructed
   * first, and destroyed (and deleted from disk) after the EdenMount is
   * destroyed.
   */
  // std::optional<folly::test::TemporaryDirectory> testDir_;
  std::filesystem::path testDir_;
  std::filesystem::path mountPath_;

  std::shared_ptr<EdenMount> edenMount_;
  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
  std::shared_ptr<EdenStats> stats_;

  std::unique_ptr<WinStore> winStore_;

  /*
   * config_ is only set before edenMount_ has been initialized.
   * When edenMount_ is created we pass ownership of the config to edenMount_.
   */
  std::unique_ptr<CheckoutConfig> config_;

  /**
   * A counter for creating temporary commit hashes via the nextCommitHash()
   * function.
   *
   * This is atomic just in case, but in general I would expect most tests to
   * perform all TestMount manipulation from a single thread.
   */
  std::atomic<uint64_t> commitNumber_{1};

  std::shared_ptr<FakeClock> clock_ = std::make_shared<FakeClock>();
  std::shared_ptr<PrivHelper> privHelper_;

  // The ManualExecutor must be destroyed prior to the EdenMount. Otherwise,
  // when clearing its queue, it will deallocate functions with captured values
  // that still reference the EdenMount (or its owned objects).
  std::shared_ptr<folly::ManualExecutor> serverExecutor_;

  std::shared_ptr<ServerState> serverState_;
};
} // namespace eden
} // namespace facebook

/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Synchronized.h>
#include <memory>
#include "eden/fs/journal/JournalDelta.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
namespace fusell {
class MountPoint;
}

class BindMount;
class ClientConfig;
class ObjectStore;
class Overlay;
class Journal;
class Tree;

/*
 * EdenMount contains all of the data about a specific eden mount point.
 *
 * This contains:
 * - The fusell::MountPoint object which manages our FUSE interactions with the
 *   kernel.
 * - The ObjectStore object used for retreiving/storing object data.
 * - The Overlay object used for storing local changes (that have not been
 *   committed/snapshotted yet).
 */
class EdenMount {
 public:
  EdenMount(
      std::shared_ptr<fusell::MountPoint> mountPoint,
      std::unique_ptr<ObjectStore> objectStore,
      std::shared_ptr<Overlay> overlay,
      std::unique_ptr<const ClientConfig> clientConfig);
  EdenMount(
      std::shared_ptr<fusell::MountPoint> mountPoint,
      std::unique_ptr<ObjectStore> objectStore,
      std::shared_ptr<Overlay> overlay,
      const std::vector<BindMount> bindMounts);
  virtual ~EdenMount();

  /*
   * Get the MountPoint object.
   *
   * This returns a raw pointer since the EdenMount owns the mount point.
   * The caller should generally maintain a reference to the EdenMount object,
   * and not directly to the MountPoint object itself.
   */
  fusell::MountPoint* getMountPoint() const {
    return mountPoint_.get();
  }

  /*
   * Return the path to the mount point.
   */
  const AbsolutePath& getPath() const;

  /*
   * Return bind mounts that are applied for this mount. These are based on the
   * state of the ClientConfig when this EdenMount was created.
   */
  const std::vector<BindMount>& getBindMounts() const;

  /*
   * Return the ObjectStore used by this mount point.
   *
   * The ObjectStore is guaranteed to be valid for the lifetime of the
   * EdenMount.
   */
  ObjectStore* getObjectStore() const {
    return objectStore_.get();
  }

  const std::shared_ptr<Overlay>& getOverlay() const {
    return overlay_;
  }

  folly::Synchronized<Journal>& getJournal() {
    return journal_;
  }

  uint64_t getMountGeneration() const {
    return mountGeneration_;
  }

  /** Convenience method for getting the Tree for the root of the mount. */
  std::unique_ptr<Tree> getRootTree() const;

 private:
  // Forbidden copy constructor and assignment operator
  EdenMount(EdenMount const&) = delete;
  EdenMount& operator=(EdenMount const&) = delete;

  std::shared_ptr<fusell::MountPoint> mountPoint_;
  std::unique_ptr<ObjectStore> objectStore_;
  std::shared_ptr<Overlay> overlay_;

  /*
   * Note that this config will not be updated if the user modifies the
   * underlying config files after the ClientConfig was created.
   */
  const std::vector<BindMount> bindMounts_;

  folly::Synchronized<Journal> journal_;

  /*
   * A number to uniquely identify this particular incarnation of this mount.
   * We use bits from the process id and the time at which we were mounted.
   */
  const uint64_t mountGeneration_;
};
}
}

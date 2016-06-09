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

#include <memory>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
namespace fusell {
class MountPoint;
}

class ObjectStore;
class Overlay;

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
      std::shared_ptr<Overlay> overlay);
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

 private:
  // Forbidden copy constructor and assignment operator
  EdenMount(EdenMount const&) = delete;
  EdenMount& operator=(EdenMount const&) = delete;

  std::shared_ptr<fusell::MountPoint> mountPoint_;
  std::unique_ptr<ObjectStore> objectStore_;
  std::shared_ptr<Overlay> overlay_;
};
}
}

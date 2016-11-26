/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenMount.h"

#include <glog/logging.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenMounts.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

using std::shared_ptr;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

// We compute this when the process is initialized, but stash a copy
// in each EdenMount.  We may in the future manage to propagate enough
// state across upgrades or restarts that we can preserve this, but
// as implemented today, a process restart will invalidate any cached
// mountGeneration that a client may be holding on to.
// We take the bottom 16-bits of the pid and 32-bits of the current
// time and shift them up, leaving 16 bits for a mount point generation
// number.
static const uint64_t globalProcessGeneration =
    (uint64_t(getpid()) << 48) | (uint64_t(time(nullptr)) << 16);

// Each time we create an EdenMount we bump this up and OR it together
// with the globalProcessGeneration to come up with a generation number
// for a given mount instance.
static std::atomic<uint16_t> mountGeneration{0};

EdenMount::EdenMount(
    shared_ptr<fusell::MountPoint> mountPoint,
    unique_ptr<ObjectStore> objectStore,
    shared_ptr<Overlay> overlay,
    unique_ptr<Dirstate> dirstate,
    const ClientConfig* clientConfig)
    : EdenMount(
          mountPoint,
          std::move(objectStore),
          overlay,
          std::move(dirstate),
          clientConfig->getBindMounts()) {}

EdenMount::EdenMount(
    shared_ptr<fusell::MountPoint> mountPoint,
    unique_ptr<ObjectStore> objectStore,
    shared_ptr<Overlay> overlay,
    std::unique_ptr<Dirstate> dirstate,
    vector<BindMount> bindMounts)
    : mountPoint_(std::move(mountPoint)),
      objectStore_(std::move(objectStore)),
      overlay_(std::move(overlay)),
      dirstate_(std::move(dirstate)),
      bindMounts_(std::move(bindMounts)),
      mountGeneration_(globalProcessGeneration | ++mountGeneration) {
  CHECK_NOTNULL(mountPoint_.get());
  CHECK_NOTNULL(objectStore_.get());
  CHECK_NOTNULL(overlay_.get());
}

EdenMount::~EdenMount() {}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

std::unique_ptr<Tree> EdenMount::getRootTree() const {
  return getRootTreeForMountPoint(mountPoint_.get(), getObjectStore());
}
}
} // facebook::eden

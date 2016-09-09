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

#include "Overlay.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

using std::shared_ptr;
using std::unique_ptr;

namespace facebook {
namespace eden {

EdenMount::EdenMount(
    shared_ptr<fusell::MountPoint> mountPoint,
    unique_ptr<ObjectStore> objectStore,
    shared_ptr<Overlay> overlay)
    : mountPoint_(std::move(mountPoint)),
      objectStore_(std::move(objectStore)),
      overlay_(std::move(overlay)) {
  CHECK_NOTNULL(mountPoint_.get());
  CHECK_NOTNULL(objectStore_.get());
  CHECK_NOTNULL(overlay_.get());
}

EdenMount::~EdenMount() {}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}
}
} // facebook::eden

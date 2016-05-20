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

#include "eden/fuse/MountPoint.h"

using std::shared_ptr;

namespace facebook {
namespace eden {

EdenMount::EdenMount(
    shared_ptr<fusell::MountPoint> mountPoint,
    shared_ptr<LocalStore> localStore,
    shared_ptr<Overlay> overlay)
    : mountPoint_(std::move(mountPoint)),
      localStore_(std::move(localStore)),
      overlay_(std::move(overlay)) {
  CHECK_NOTNULL(mountPoint_.get());
  CHECK_NOTNULL(localStore_.get());
  CHECK_NOTNULL(overlay_.get());
}

EdenMount::~EdenMount() {}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}
}
} // facebook::eden

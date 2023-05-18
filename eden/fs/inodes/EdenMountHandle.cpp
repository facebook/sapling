/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/EdenMountHandle.h"
#include "eden/fs/inodes/EdenMount.h"

namespace facebook::eden {

ObjectStore& EdenMountHandle::getObjectStore() const {
  return *edenMount_->getObjectStore();
}

const std::shared_ptr<ObjectStore>& EdenMountHandle::getObjectStorePtr() const {
  return edenMount_->getObjectStore();
}

Journal& EdenMountHandle::getJournal() const {
  return edenMount_->getJournal();
}

} // namespace facebook::eden

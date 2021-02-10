/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/EdenDispatcherFactory.h"

#ifndef _WIN32
#include "eden/fs/inodes/FuseDispatcherImpl.h"
#else
#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#endif

namespace facebook::eden {

#ifndef _WIN32
std::unique_ptr<FuseDispatcher> EdenDispatcherFactory::makeFuseDispatcher(
    EdenMount* mount) {
  return std::make_unique<FuseDispatcherImpl>(mount);
}
#else
std::unique_ptr<PrjfsDispatcher> EdenDispatcherFactory::makePrjfsDispatcher(
    EdenMount* mount) {
  return std::make_unique<PrjfsDispatcherImpl>(mount);
}
#endif

} // namespace facebook::eden

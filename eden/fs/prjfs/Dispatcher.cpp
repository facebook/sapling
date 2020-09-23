/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/prjfs/Dispatcher.h"

namespace facebook::eden {
Dispatcher::~Dispatcher() {}

Dispatcher::Dispatcher(EdenStats* stats) : stats_(stats) {}

EdenStats* Dispatcher::getStats() const {
  return stats_;
}
} // namespace facebook::eden

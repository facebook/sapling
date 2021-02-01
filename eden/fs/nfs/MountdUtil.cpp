/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/io/async/EventBaseManager.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include "eden/fs/nfs/Mountd.h"

using namespace facebook::eden;

FOLLY_INIT_LOGGING_CONFIG("eden=INFO");

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  Mountd mountd;

  folly::EventBaseManager::get()->getEventBase()->loopForever();
}

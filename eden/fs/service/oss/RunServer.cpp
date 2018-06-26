/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/EdenServer.h"

#include <thrift/lib/cpp2/server/ThriftServer.h>

namespace facebook {
namespace eden {

std::string getEdenfsBuildName() {
  // We don't have any version information for now, so just return "edenfs"
  return "edenfs";
}

void runServer(const EdenServer& server) {
  // ThriftServer::serve() will drive the current thread's EventBase.
  // Verify that we are being called from the expected thread, and will end up
  // driving the EventBase returned by EdenServer::getMainEventBase().
  CHECK_EQ(
      server.getMainEventBase(),
      folly::EventBaseManager::get()->getEventBase());
  server.getServer()->serve();
}
} // namespace eden
} // namespace facebook

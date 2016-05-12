/*
 *  Copyright (c) 2016, Facebook, Inc.
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
void EdenServer::runThriftServer() {
  server_->serve();
}
}
} // facebook::eden

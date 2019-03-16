/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <string>

namespace facebook {
namespace eden {

class EdenServer;

class EdenMain {
 public:
  virtual ~EdenMain() {}
  int main(int argc, char** argv);

 protected:
  // Subclasses can override these methods to tweak Eden's start-up behavior
  virtual std::string getEdenfsBuildName();
  virtual void runServer(const EdenServer& server);
};

} // namespace eden
} // namespace facebook

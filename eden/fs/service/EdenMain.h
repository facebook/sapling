/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

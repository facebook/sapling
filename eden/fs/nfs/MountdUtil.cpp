/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/io/async/EventBaseManager.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <signal.h>
#include "eden/fs/nfs/NfsServer.h"

using namespace facebook::eden;

FOLLY_INIT_LOGGING_CONFIG("eden=INFO");

namespace {
class SignalHandler : public folly::AsyncSignalHandler {
 public:
  explicit SignalHandler(folly::EventBase* evb)
      : folly::AsyncSignalHandler(evb) {
    registerSignalHandler(SIGTERM);
    registerSignalHandler(SIGINT);
  }

 private:
  void signalReceived(int signum) noexcept override {
    if (signum == SIGTERM || signum == SIGINT) {
      getEventBase()->terminateLoopSoon();
    }
  }
};
} // namespace

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  auto evb = folly::EventBaseManager::get()->getEventBase();

  SignalHandler signal(evb);
  NfsServer server(false, evb);
  auto [nfsd, mountdport, nfsdport] = server.registerMount(
      AbsolutePathPiece("/foo/bar"),
      InodeNumber(42),
      nullptr,
      nullptr,
      nullptr,
      std::chrono::duration_cast<folly::Duration>(std::chrono::seconds(0)),
      nullptr);

  XLOG(INFO) << "Started NfsServer, mountdport=" << mountdport
             << ", nfsdport=" << nfsdport;

  evb->loop();
}

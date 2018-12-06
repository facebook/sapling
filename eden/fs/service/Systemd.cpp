/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/Systemd.h"
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include "eden/fs/eden-config.h"

#if EDEN_HAVE_SYSTEMD
#include <systemd/sd-daemon.h>
#endif

namespace facebook {
namespace eden {

#if EDEN_HAVE_SYSTEMD
DEFINE_bool(
    experimentalSystemd,
    false,
    "EXPERIMENTAL: Run edenfs as if systemd controls its lifecycle");

void Systemd::notifyReady() {
  // TODO(strager): Move READY=1 into a systemd-specific StartupLogger.
  auto rc = sd_notify(/*unset_environment=*/false, "READY=1");
  if (rc < 0) {
    XLOG(ERR) << "sd_notify READY=1 failed: " << folly::errnoStr(-rc);
  } else if (rc == 0) {
    XLOG(WARN)
        << "sd_notify READY=1 failed: $NOTIFY_SOCKET is unset. edenfs was probably not started by systemd.";
  }
}
#endif

} // namespace eden
} // namespace facebook

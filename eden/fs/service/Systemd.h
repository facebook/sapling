/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <gflags/gflags.h>
#include "eden/fs/eden-config.h"

namespace facebook {
namespace eden {

#if EDEN_HAVE_SYSTEMD
DECLARE_bool(experimentalSystemd);

class Systemd {
 public:
  static void notifyReady();
};
#endif

} // namespace eden
} // namespace facebook

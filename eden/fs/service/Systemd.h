/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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

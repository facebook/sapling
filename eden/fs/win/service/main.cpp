/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Conv.h>
#include <folly/experimental/FunctionScheduler.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <iostream>
#include <memory>
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/win/service/WinService.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/io/IOBuf.h"
#include "folly/portability/Windows.h"

#ifndef _WIN32
#error This is a Windows only source file;
#endif

using namespace facebook::eden;
using namespace std;
using namespace folly;

// The --edenfs flag is defined to help make the flags consistent across Windows
// and non-Windows platforms.  On non-Windows platform this flag is required, as
// a check to help ensure that users do not accidentally invoke `edenfs` when
// they meant to run `edenfsctl`.
// It probably would be nice to eventually require this behavior on Windows too.
DEFINE_bool(
    edenfs,
    false,
    "This optional argument is currently ignored on Windows");

int __cdecl main(int argc, char** argv) {
  folly::init(&argc, &argv);
  WinService::create(argc, argv);
}

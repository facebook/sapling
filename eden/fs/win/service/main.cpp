/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenMain.h"

using namespace facebook::eden;

int __cdecl main(int argc, char** argv) {
  try {
    return runEdenMain(DefaultEdenMain{}, argc, argv);
  } catch (const std::exception& ex) {
    fprintf(stderr, "Error while running EdenFS: %s\n", ex.what());
    return 1;
  }
}

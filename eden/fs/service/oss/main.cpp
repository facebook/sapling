/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenMain.h"

int main(int argc, char** argv) {
  facebook::eden::EdenMain server;
  return server.main(argc, argv);
}

/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/Hash.h"

#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <sysexits.h>

#include "HgImporter.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/utils/PathFuncs.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(rev, "", "The revision ID to import");

using namespace facebook::eden;

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);

  if (argc != 2) {
    fprintf(stderr, "usage: hg_import --edenDir=<dir> <repository>\n");
    return EX_USAGE;
  }
  auto repoPath = argv[1];
  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: --edenDir must be specified\n");
    return EX_USAGE;
  }
  auto rocksPath =
      canonicalPath(FLAGS_edenDir) + RelativePathPiece{"storage/rocks-db"};

  std::string revName = FLAGS_rev;
  if (revName.empty()) {
    revName = ".";
  }

  LocalStore store(rocksPath);
  HgImporter importer(repoPath, &store);
  auto rootHash = importer.importManifest(revName);
  printf("Imported root tree: %s\n", rootHash.toString().c_str());

  return EX_OK;
}

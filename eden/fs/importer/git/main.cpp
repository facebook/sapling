/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/importer/git/GitImporter.h"

#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <iostream>

using facebook::eden::doGitImport;

static bool validateRequiredString(const char* flagname,
                                   const std::string& value) {
  if (value.empty()) {
    std::cerr << "--" << flagname << " is a required argument\n";
    return false;
  } else {
    return true;
  }
}
DEFINE_string(repo, "", "location of the Git repository");
DEFINE_string(db, "", "location of the RocksDB");
static const bool repo_dummy =
    google::RegisterFlagValidator(&FLAGS_repo, &validateRequiredString);
static const bool db_dummy =
    google::RegisterFlagValidator(&FLAGS_db, &validateRequiredString);

/**
 * Utility to import the contents of a .git directory into a RocksDB.
 */
int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  // TODO(wez): Is there a way to avoid --help printing so much junk,
  // and why does gflags seem to use single-hyphen args for its built-in flags?
  google::SetUsageMessage("Usage: specify a --repo and a --db");
  google::ParseCommandLineFlags(&argc, &argv, true);
  doGitImport(FLAGS_repo, FLAGS_db);
}

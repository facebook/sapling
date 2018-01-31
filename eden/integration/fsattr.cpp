/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/dynamic.h>
#include <folly/init/Init.h>
#include <folly/json.h>
#include <gflags/gflags.h>
#include "eden/fs/utils/XAttr.h"

DEFINE_string(fileName, "", "the path to examine");
DEFINE_string(
    attrName,
    "",
    "the name of the attribute to return, else list all of them");

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  folly::dynamic result;

  if (FLAGS_attrName.empty()) {
    // List attributes

    result = folly::dynamic::object();

    for (auto& name : facebook::eden::listxattr(FLAGS_fileName)) {
      auto value = facebook::eden::getxattr(FLAGS_fileName, name);
      result.insert(name, value);
    }

  } else {
    // Return named attribute
    result = facebook::eden::getxattr(FLAGS_fileName, FLAGS_attrName);
  }

  auto serialized =
      folly::json::serialize(result, folly::json::serialization_opts());
  fwrite(serialized.data(), 1, serialized.size(), stdout);
  puts("\n");

  return 0;
}

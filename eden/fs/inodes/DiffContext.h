/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

namespace facebook {
namespace eden {

/**
 * A small helper class to store parameters for a TreeInode::diff() operation.
 *
 * These are parameters that remain fixed across all subdirectories being
 * diffed.  This class is mostly just for convenience so that we do not have to
 * pass these items in individually as separate parameters to each function
 * being called.
 */
class DiffContext {
 public:
  DiffContext(InodeDiffCallback* cb, bool listIgn, ObjectStore* os)
      : callback{cb}, store{os}, listIgnored{listIgn} {}

  InodeDiffCallback* const callback;
  ObjectStore* const store;
  /**
   * If listIgnored is true information about ignored files will be reported.
   * If listIgnored is false then ignoredFile() will never be called on the
   * callback.  The diff operation may be faster with listIgnored=false, since
   * it can completely omit processing ignored subdirectories.
   */
  bool const listIgnored;
};
}
}

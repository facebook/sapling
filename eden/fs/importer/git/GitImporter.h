/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <string>

namespace facebook {
namespace eden {

/**
 * @param repoPath must be a path to an existing Git repository.
 * @param dbPath must be a path. The DB will be created if it does not already
 *   exist.
 */
std::string doGitImport(const std::string& repoPath, const std::string& dbPath);
}
}

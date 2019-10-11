/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

void createRepoConfig(
    const AbsolutePath& repoPath,
    const AbsolutePath& socket,
    const AbsolutePath& client);
}
} // namespace facebook

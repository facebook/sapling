/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <iosfwd>
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class EdenMount;

/**
 * Returns the single-char representation for the ScmFileStatus used by
 * SCMs such as Git and Mercurial.
 */
char scmStatusCodeChar(ScmFileStatus code);

std::ostream& operator<<(std::ostream& os, const ScmStatus& status);

folly::Future<std::unique_ptr<ScmStatus>>
diffMountForStatus(const EdenMount& mount, Hash commitHash, bool listIgnored);

} // namespace eden
} // namespace facebook

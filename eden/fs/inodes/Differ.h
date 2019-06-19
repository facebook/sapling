/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <iosfwd>
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"

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

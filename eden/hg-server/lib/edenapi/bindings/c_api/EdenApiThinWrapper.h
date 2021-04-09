/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <memory>

#include "eden/hg-server/lib/edenapi/bindings/c_api/RustEdenApi.h"

namespace folly {
class IOBuf;
} // namespace folly

namespace facebook {
namespace eden {

class EdenApiClient {};

class CppKey {};

class TreeEntryFetch {};

class TreeChildRef {};

} // namespace eden
} // namespace facebook

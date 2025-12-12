/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

class SaplingBackingStoreOptions {
  // All runtime options have been removed, but it's worth keeping this around
  // for easy extensibility in the future
 public:
  /* implicit */ SaplingBackingStoreOptions() {}
};

} // namespace facebook::eden

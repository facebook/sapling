/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/scm/lib/backingstore/include/SaplingBackingStoreError.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h" // @manual

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <rust/cxx.h>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <type_traits>

namespace sapling {

SaplingNativeBackingStore::SaplingNativeBackingStore(
    std::string_view repository,
    std::string_view mount,
    facebook::eden::HgObjectIdFormat objectIdFormat,
    facebook::eden::CaseSensitivity caseSensitive)
    : store_{
          sapling_backingstore_new(
              rust::Slice<const char>{repository.data(), repository.size()},
              rust::Slice<const char>{mount.data(), mount.size()})
              .into_raw(),
          [](BackingStore* backingStore) {
            auto box = rust::Box<BackingStore>::from_raw(backingStore);
          }}, objectIdFormat_{objectIdFormat}, caseSensitive_{caseSensitive} {
  try {
    repoName_ = std::string(sapling_backingstore_get_name(*store_.get()));
  } catch (const rust::Error& error) {
    XLOGF(DBG2, "Error while repo name from backingstore: {}", error.what());
  }
}

} // namespace sapling

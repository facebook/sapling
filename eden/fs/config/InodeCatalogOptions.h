/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Utility.h>
#include <cstdint>
#include <type_traits>
#include "eden/common/utils/OptionSet.h"

namespace facebook::eden {

/**
 * Options for `InodeCatalogType`s. Currently only used by `Sqlite`.
 * Multiple values can be OR'd together. DEFAULT should be used to
 * signal no options are enabled.
 */
struct InodeCatalogOptions : OptionSet<InodeCatalogOptions, uint32_t> {
  using OptionSet::OptionSet;
  static const NameTable table;
};

constexpr inline auto INODE_CATALOG_DEFAULT = InodeCatalogOptions::raw(0);
constexpr inline auto INODE_CATALOG_UNSAFE_IN_MEMORY =
    InodeCatalogOptions::raw(1);
constexpr inline auto INODE_CATALOG_SYNCHRONOUS_OFF =
    InodeCatalogOptions::raw(2);
constexpr inline auto INODE_CATALOG_BUFFERED = InodeCatalogOptions::raw(4);

} // namespace facebook::eden

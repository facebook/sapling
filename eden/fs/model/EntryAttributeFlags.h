/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/utils/OptionSet.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook::eden {

struct EntryAttributeFlags
    : OptionSet<EntryAttributeFlags, std::underlying_type_t<FileAttributes>> {
  constexpr static EntryAttributeFlags raw(FileAttributes raw) {
    return OptionSet<
        EntryAttributeFlags,
        std::underlying_type_t<FileAttributes>>::raw(folly::to_underlying(raw));
  }
  constexpr static EntryAttributeFlags raw(
      std::underlying_type_t<FileAttributes> raw) {
    return OptionSet<
        EntryAttributeFlags,
        std::underlying_type_t<FileAttributes>>::raw(raw);
  }
};

inline constexpr auto ENTRY_ATTRIBUTE_TYPE =
    EntryAttributeFlags::raw(FileAttributes::SOURCE_CONTROL_TYPE);
inline constexpr auto ENTRY_ATTRIBUTE_SIZE =
    EntryAttributeFlags::raw(FileAttributes::FILE_SIZE);
inline constexpr auto ENTRY_ATTRIBUTE_SHA1 =
    EntryAttributeFlags::raw(FileAttributes::SHA1_HASH);

} // namespace facebook::eden

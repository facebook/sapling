/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string_view>

namespace facebook::eden {

enum class EdenComponent {
  Fuse,
  Nfs,
  Prjfs,
  Overlay,
  BackingStore,
  ObjectStore,
  Thrift,
  Takeover,
  Privhelper,
};

std::string_view toString(EdenComponent component);

} // namespace facebook::eden

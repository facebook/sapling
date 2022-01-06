/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include "common/rust/srserver/src/Metadata.h"

namespace facebook {
namespace scm {
namespace service {

std::unique_ptr<rust::srserver::RustThriftMetadata> create_metadata() noexcept;

} // namespace service
} // namespace scm
} // namespace facebook

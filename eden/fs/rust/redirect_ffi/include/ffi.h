/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/rust/redirect_ffi/src/lib.rs.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook::eden {

Redirection redirectionFromFFI(RedirectionFFI&& redirFFI);

std::optional<std::string> redirectionTargetFromFFI(rust::String&& targetFFI);
} // namespace facebook::eden

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

constexpr const char* kDisablePrivHelperHardeningPath{
    "/etc/eden/disable_privhelper_hardening"};

bool disablePrivHelperHardening();

} // namespace facebook::eden

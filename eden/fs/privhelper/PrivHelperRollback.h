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

/**
 * Check the root-controlled rollback marker and minimum supported kernel.
 * Restart the helper when changing the marker to apply startup credentials.
 */
bool disablePrivHelperHardening();

} // namespace facebook::eden

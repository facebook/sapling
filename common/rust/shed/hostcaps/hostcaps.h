/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

#pragma once

// @dep=//common/rust/shed/hostcaps:hostcaps

extern "C" uint8_t fb_get_env();
extern "C" bool fb_is_prod();
extern "C" bool fb_is_corp();
extern "C" bool fb_is_lab();
extern "C" bool fb_has_servicerouter();

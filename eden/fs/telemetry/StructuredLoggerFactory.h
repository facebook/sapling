/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

namespace facebook::eden {

class EdenConfig;
class StructuredLogger;
struct SessionInfo;

/**
 * Returns a StructuredLogger appropriate for this platform and Eden
 * configuration.
 */
std::shared_ptr<StructuredLogger> makeDefaultStructuredLogger(
    const EdenConfig&,
    SessionInfo sessionInfo);

} // namespace facebook::eden

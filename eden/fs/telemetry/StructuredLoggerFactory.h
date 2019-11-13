/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

namespace facebook {
namespace eden {

class EdenConfig;
class StructuredLogger;
struct SessionInfo;

/**
 * Returns a StructuredLogger appropriate for this platform and Eden
 * configuration.
 */
std::unique_ptr<StructuredLogger> makeDefaultStructuredLogger(
    const EdenConfig&,
    SessionInfo sessionInfo);

} // namespace eden
} // namespace facebook

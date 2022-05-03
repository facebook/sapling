/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#ifdef _WIN32

namespace facebook::eden {
void installWindowsExceptionFilter();

void printCurrentStack();
} // namespace facebook::eden
#endif

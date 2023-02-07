/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef _WIN32

namespace folly {
template <typename T>
class Try;
}

namespace facebook::eden {

folly::Try<bool> isRenamedPlaceholder(const wchar_t* path);

} // namespace facebook::eden

#endif

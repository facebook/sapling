/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// Requires C++20
#if 0
#include <version>
#endif

// Temporarily disabled because our version below uses std::string_view to avoid
// call sites needing to strlen in, for example, log formatting.
// We could consider a thin wrapper around std::source_location that caches the
// file and function name lengths.
#if 0 && defined(__cpp_lib_source_location)

#include <source_location>

namespace facebook::eden {

using SourceLocation = std::source_location;

#define EDEN_CURRENT_SOURCE_LOCATION (::std::source_location::current())

} // namespace facebook::eden

#else

#include <stdint.h>
#include <string_view>

namespace facebook::eden {

/**
 * Pointer-sized reference to an EDEN_CURRENT_SOURCE_LOCATION call-site.
 *
 * Replacement for std::source_location until we can rely on C++20 on all
 * platforms. Intentionally uses std::string_view because __FILE__ and
 * __func__'s lengths are known at compile-time and std::source_location
 * requires call sites compute lengths dynamically with strlen.
 */
class SourceLocation {
 public:
  using line_t = uint32_t;

  SourceLocation() = delete;
  SourceLocation(const SourceLocation&) = default;
  SourceLocation(SourceLocation&&) = default;
  SourceLocation& operator=(const SourceLocation&) = default;
  SourceLocation& operator=(SourceLocation&&) = default;

  std::string_view function_name() const noexcept {
    return record_->function_name;
  }

  std::string_view file_name() const noexcept {
    return record_->file_name;
  }

  line_t line() const noexcept {
    return record_->line;
  }

 public: // Public only for CURRENT_SOURCE_LOCATION
  struct Record {
    std::string_view function_name;
    std::string_view file_name;
    line_t line;
  };

  explicit SourceLocation(const Record* record) noexcept : record_{record} {}

 private:
  const Record* record_;
};

// If we can use gcc's nonstandard statement expressions, much better code is
// generated.
#if defined(__GNUC__) || defined(__clang__)

/// Returns a `SourceLocation` corresponding to the call site.
#define EDEN_CURRENT_SOURCE_LOCATION                                     \
  ({                                                                     \
    static const SourceLocation::Record s{__func__, __FILE__, __LINE__}; \
    SourceLocation{&s};                                                  \
  })

#else

/**
 * Returns a `SourceLocation` corresponding to the call site.
 * TODO: MSVC generates bad code for this pattern. It does not see the static
 * can be allocated in the constant data section, so it does a thread-safe
 * initialization. It's possible to shorten the generated code in MSVC by
 * initializing a non-const static with constant data, and always assigning func
 * on access. This removes the thread-safe initialization logic and replaces it
 * with a single store on every use.
 */
#define EDEN_CURRENT_SOURCE_LOCATION                                 \
  ([](std::string_view func) {                                       \
    static const SourceLocation::Record s{func, __FILE__, __LINE__}; \
    return SourceLocation{&s};                                       \
  }(__func__))

#endif

} // namespace facebook::eden

#endif

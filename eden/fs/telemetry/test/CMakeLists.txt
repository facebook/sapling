# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)

# These tests check the function names in captured stack traces, which the Rust
# backtrace crate finds on macOS only in a .dSYM.
if (APPLE)
  foreach(test IN ITEMS
      eden_fs_telemetry_test_throw_trace_capture_test
      eden_fs_telemetry_test_error_info_builder_test)
    add_custom_command(
      TARGET ${test} POST_BUILD COMMAND dsymutil "$<TARGET_FILE:${test}>"
    )
  endforeach()
endif()

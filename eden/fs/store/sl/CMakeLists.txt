# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)

if (BUILD_TESTS AND NOT WIN32)
 add_subdirectory(test)
endif()

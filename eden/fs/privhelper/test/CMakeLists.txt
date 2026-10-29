# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)

add_executable(drop_privs DropPrivs.cpp)
target_link_libraries(drop_privs eden_fuse_privhelper Folly::folly)

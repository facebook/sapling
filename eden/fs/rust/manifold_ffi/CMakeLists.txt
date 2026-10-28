# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

rust_cxx_bridge(manifold_ffi "src/lib.rs")
target_link_libraries(manifold_ffi PUBLIC rust_edenfs_staticlib)

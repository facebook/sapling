# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

rust_cxx_bridge(redirect_ffi "src/lib.rs")
target_sources(redirect_ffi PRIVATE src/ffi.cpp)
target_link_libraries(redirect_ffi PUBLIC eden_service_thrift_cpp)

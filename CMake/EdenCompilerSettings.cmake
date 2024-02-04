# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if (WIN32)
  set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -DWIN32_LEAN_AND_MEAN -DNOMINMAX -DSTRICT")
elseif (APPLE)
  set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wno-nullability-completeness -Wno-unknown-pragmas")
else()
  set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wno-unknown-pragmas")
endif()

# TODO: this should be a configure-time check. Linux and libstdc++
# requires explicitly linking libatomic.
if (NOT WIN32 AND NOT APPLE)
  link_libraries(atomic)
endif()

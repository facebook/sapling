# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

set(CMAKE_CXX_FLAGS_COMMON "-g -Wall -Wextra -Wno-deprecated -Wno-deprecated-declarations")
set(CMAKE_CXX_FLAGS_DEBUG "${CMAKE_CXX_FLAGS_DEBUG} ${CMAKE_CXX_FLAGS_COMMON}")
set(CMAKE_CXX_FLAGS_RELEASE "${CMAKE_CXX_FLAGS_RELEASE} ${CMAKE_CXX_FLAGS_COMMON} -O3")

function(apply_eden_compile_options_to_target THETARGET)
  target_compile_options(${THETARGET}
    PUBLIC
      -g
      -finput-charset=UTF-8
      -fsigned-char
      -Werror
      -Wall
      -Wno-deprecated
      -Wno-deprecated-declarations
      -Wno-error=deprecated-declarations
      -Wno-sign-compare
      -Wno-unused
      -Wunused-label
      -Wunused-result
      -Wnon-virtual-dtor
      ${FOLLY_CXX_FLAGS}
    PRIVATE
      -D_REENTRANT
      -D_GNU_SOURCE
  )
endfunction()

# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.
#
# Find cpptoml
#
# This package sets:
# CPPTOML_FOUND - Whether cpptoml was found
# CPPTOML_INCLUDE_DIR - The include directory for cpptoml
# cpptoml is a header-only library, so there is no CPPTOML_LIBRARY variable.
# 
include(FindPackageHandleStandardArgs)

find_path(CPPTOML_INCLUDE_DIR NAMES cpptoml.h)
find_package_handle_standard_args(
  CPPTOML
  DEFAULT_MSG
  CPPTOML_INCLUDE_DIR
)
mark_as_advanced(CPPTOML_INCLUDE_DIR)

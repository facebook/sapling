# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FindPackageHandleStandardArgs)

find_path(CPPTOML_INCLUDE_DIR NAMES cpptoml.h)
find_package_handle_standard_args(
  CPPTOML
  DEFAULT_MSG
  CPPTOML_INCLUDE_DIR
)
mark_as_advanced(CPPTOML_INCLUDE_DIR)

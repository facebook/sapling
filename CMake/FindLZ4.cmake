# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

find_path(LZ4_INCLUDE_DIR NAMES lz4.h)

find_library(LZ4_LIBRARY_DEBUG NAMES lz4d)
find_library(LZ4_LIBRARY_RELEASE NAMES lz4)

include(SelectLibraryConfigurations)
select_library_configurations(LZ4)

include(FindPackageHandleStandardArgs)
find_package_handle_standard_args(
    LZ4 DEFAULT_MSG
    LZ4_LIBRARY LZ4_INCLUDE_DIR
)

mark_as_advanced(LZ4_INCLUDE_DIR LZ4_LIBRARY)

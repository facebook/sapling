# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if (WIN32)
  file(GLOB PRJFS_SRCS "*.cpp" "*.h")

  add_library(
    eden_prjfs STATIC
      ${PRJFS_SRCS}
  )

  target_link_libraries(
    eden_prjfs
    PUBLIC
      eden_request_context
      eden_inodes_inodenumber
      eden_journal
      eden_model
      eden_store
      eden_telemetry
      eden_utils
      eden_service_thrift_util
      Folly::folly
      ProjectedFS
      cpptoml
      edencommon::edencommon_utils
  )
endif()

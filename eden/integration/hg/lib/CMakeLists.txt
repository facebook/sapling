# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fb_python_library(
  eden_hg_integration_lib
  SOURCES
    hg_extension_test_base.py
    histedit_command.py
  DEPENDS
    eden_integration_lib
)

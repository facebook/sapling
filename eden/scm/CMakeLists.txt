# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fb_python_library(
  eden_scm_redact
  NAMESPACE eden.scm
  SOURCES
    sapling/redact.py
)
install_fb_python_library(eden_scm_redact)

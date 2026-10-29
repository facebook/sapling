# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

rust_executable(edenfsctl ENV ${EDEN_RUST_THRIFT_ENV})
if (EDEN_HAVE_THRIFT_PYTHON)
  add_dependencies(edenfsctl.cargo edenfsctl.real.GEN_PY_EXE)
endif()
install_rust_executable(edenfsctl)

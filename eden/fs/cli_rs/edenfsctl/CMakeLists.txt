# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

rust_executable(edenfsctl)
add_dependencies(edenfsctl.cargo edenfsctl.real.GEN_PY_EXE)
install_rust_executable(edenfsctl)

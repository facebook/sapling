# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

help:
	@echo 'This Makefile is supposed to be used by'
	@echo 'fbcode_builder/getdeps.py script, DO NOT use it directly.'
	@echo 'Instead please refer to run_tests_getdeps.py for instructions'
	@echo 'on how to run tests manually with a getdeps builded code'

all: help

build-getdeps:
	mkdir -p $(GETDEPS_BUILD_DIR)/mononoke_integration
	@echo "building..."
	touch $(GETDEPS_BUILD_DIR)/mononoke_integration/build

install-getdeps:
	mkdir -p $(GETDEPS_INSTALL_DIR)/mononoke_integration
	@echo "installing..."
	touch $(GETDEPS_BUILD_DIR)/mononoke_integration/install

.PHONY: help all build-getdeps install-getdeps

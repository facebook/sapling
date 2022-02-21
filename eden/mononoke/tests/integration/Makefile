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
	# In this step just generate the manifest.json file
	./run_tests_getdeps.py getdeps $(GETDEPS_INSTALL_DIR) --generate_manifest

install-getdeps:
	mkdir -p $(GETDEPS_INSTALL_DIR)/mononoke_integration
	# In this step copy the integration/ folder and the manifest.json file
	# to the installation directory
	cp -r ../ $(GETDEPS_INSTALL_DIR)/mononoke_integration
	cp $(GETDEPS_BUILD_DIR)/mononoke_integration/manifest.json $(GETDEPS_INSTALL_DIR)/mononoke_integration

test-getdeps:
	# Custom tmp folder inside getdeps scratch path, just to make sure it
	# has all proper permissions
	mkdir -p $(GETDEPS_BUILD_DIR)/mononoke_integration/tests-tmp
	# Remove the .testfailed and .testerrored files so that after this next
	# step they are written clean
	rm -f $(GETDEPS_INSTALL_DIR)/mononoke/source/eden/mononoke/tests/integration/.test*
	# Unsetting http_proxy and https_proxy, because all the traffic from
	# tests go to localhost (and for some reason the no_proxy=localhost env
	# variable is not respected).
	unset http_proxy; \
	  unset https_proxy; \
	  export TMPDIR=$(GETDEPS_BUILD_DIR)/mononoke_integration/tests-tmp; \
	  export GETDEPS_BUILD=1; \
	  ./run_tests_getdeps.py getdeps $(GETDEPS_INSTALL_DIR) || true
	# Rerunning the failed test again, because with so many tests run
	# concurrently there is a certain amount of flakiness involved.
	cat $(GETDEPS_INSTALL_DIR)/mononoke/source/eden/mononoke/tests/integration/.test* || true
	unset http_proxy; \
	  unset https_proxy; \
	  export TMPDIR=$(GETDEPS_BUILD_DIR)/mononoke_integration/tests-tmp; \
	  export GETDEPS_BUILD=1; \
	  ./run_tests_getdeps.py getdeps $(GETDEPS_INSTALL_DIR) --rerun-failed

.PHONY: help all build-getdeps install-getdeps test-getdeps

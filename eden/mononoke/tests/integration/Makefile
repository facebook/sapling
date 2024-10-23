# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

MAKE_PID := $(shell echo $$PPID)
JOBS := $(shell ps T | sed -n 's%.*$(MAKE_PID).*$(MAKE).* \(-j\|--jobs=\) *\([0-9][0-9]*\).*%\2%p')

PYTHON_SYS_EXECUTABLE=$(shell python3 ../../../scm/contrib/pick_python.py python3)

help:
	@echo 'This Makefile is supposed to be used by'
	@echo 'fbcode_builder/getdeps.py script, DO NOT use it directly.'
	@echo 'Instead please refer to run_tests_getdeps.py for instructions'
	@echo 'on how to run tests manually with a getdeps builded code'

all: help

build-getdeps:
	mkdir -p $(GETDEPS_BUILD_DIR)/mononoke_integration
	# In this step just generate the manifest.json file
	PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
	$(PYTHON_SYS_EXECUTABLE) ./run_tests_getdeps.py getdeps --jobs $(JOBS) $(GETDEPS_INSTALL_DIR) --generate_manifest

install-getdeps:
	mkdir -p $(GETDEPS_INSTALL_DIR)/mononoke_integration
	# In this step copy the integration/ folder and the manifest.json file
	# to the installation directory
	cp -r ../ $(GETDEPS_INSTALL_DIR)/mononoke/source/eden/mononoke/tests
	cp $(GETDEPS_BUILD_DIR)/mononoke_integration/manifest.json $(GETDEPS_INSTALL_DIR)/mononoke_integration

test-getdeps:
	# Custom tmp folder inside getdeps scratch path, just to make sure it
	# has all proper permissions
	mkdir -p $(GETDEPS_BUILD_DIR)/mononoke_integration/tests-tmp
	# Remove the .testfailed and .testerrored files so that after this next
	# step they are written clean
	rm -f $(GETDEPS_INSTALL_DIR)/mononoke/source/eden/scm/tests/.test*
	# Unsetting http_proxy and https_proxy, because all the traffic from
	# tests go to localhost (and for some reason the no_proxy=localhost env
	# variable is not respected).
	# Allow rerunning any failed test again
	unset http_proxy; \
	  unset https_proxy; \
	  export TMPDIR=$(GETDEPS_BUILD_DIR)/mononoke_integration/tests-tmp; \
	  export GETDEPS_BUILD=1; \
	  for try in $$(seq 0 $(GETDEPS_TEST_RETRY)); do \
	    RERUN_ARG=""; \
	    if [ $$try -gt 0 ]; then RERUN_ARG="--rerun-failed"; fi; \
	    	PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
			$(PYTHON_SYS_EXECUTABLE) ./run_tests_getdeps.py getdeps --jobs $(JOBS) $(GETDEPS_INSTALL_DIR) $(GETDEPS_TEST_FILTER) $$RERUN_ARG; \
		status=$$?; \
		# stop if all good \
		if [ $$status = 0 ]; then echo "passed on try $$try"; exit 0; fi; \
	  done; \
	  exit $$status

.PHONY: help all build-getdeps install-getdeps test-getdeps

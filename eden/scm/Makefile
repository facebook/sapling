# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# If you want to change PREFIX, do not just edit it below. The changed
# value won't get passed on to recursive make calls. You should instead
# override the variable on the command like:
#
# % make PREFIX=/opt/ install

export PREFIX=/usr/local

ifeq ($(OS),Windows_NT)
PYTHON := python
endif
PYTHON3 := python3

PYTHON_SYS_EXECUTABLE=$(shell $(PYTHON3) contrib/pick_python.py $(PYTHON3))

BUCK_BUILD_MODE = @fbcode//mode/opt
ifeq ($(OS),Windows_NT)
  BUCK_BUILD_MODE := @fbcode//mode/opt-win
else
  UNAME_S := $(shell uname -s)
  ifeq ($(UNAME_S),Darwin)
    BUCK_BUILD_MODE := @fbcode//mode/opt-mac
  endif
endif

PYTHON_MINOR_VERSION=$(shell $(PYTHON_SYS_EXECUTABLE) -c "import sys; print(sys.version_info.minor)")

BUILD_PY = $(PYTHON_SYS_EXECUTABLE) ./build.py
BUILD_MODE_FLAG = $(if $(OSS),--oss,)
BUILD_ARGS = $(BUILD_MODE_FLAG) --with-python $(PYTHON_SYS_EXECUTABLE)

$(eval HGROOT := $(shell pwd))
HGPYTHONS ?= $(HGROOT)/build/pythons
PURE=
export LANGUAGE=C
export LC_ALL=C
TESTFLAGS ?= $(shell echo $$HGTESTFLAGS)
OSXVERSIONFLAGS ?= $(shell echo $$OSXVERSIONFLAGS)

HGNAME ?= hg
ifeq ($(OS),Windows_NT)
HG_BIN_NAME ?= hg.exe
SL_BIN_NAME ?= sl.exe
else
HG_BIN_NAME ?= hg
SL_BIN_NAME ?= sl
endif
SL_NAME = sl
ISL_NAME ?= isl-dist.tar.xz

MAKE_PID := $(shell echo $$PPID)
JOBS := $(shell ps T | sed -n -Ee 's%.*$(MAKE_PID).*$(MAKE).* (-j|--jobs=) *([0-9][0-9]*).*%\2%p')

# Mac Big Sur doesn't find the standard library without this.
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk

help:
	@echo 'Commonly used make targets:'
	@echo '  local        - build for inplace usage'
	@echo '  tests        - run all tests in the automatic test suite'
	@echo
	@echo 'Example for a local installation (usable in this directory):'
	@echo '  make sl && ./sl version'

all: build

oss: OSS=true
oss: HGNAME=$(SL_NAME)
oss: HG_BIN_NAME=$(SL_BIN_NAME)
oss: local

install-oss: oss
	$(SL_NAME) --kill-chg-daemon || true
	mkdir -p $(DESTDIR)/$(PREFIX)/bin
	cp out/$(SL_BIN_NAME) $(DESTDIR)/$(PREFIX)/bin/$(SL_BIN_NAME)
	mkdir -p $(DESTDIR)/$(PREFIX)/lib
	cp out/isl-dist.tar.xz $(DESTDIR)/$(PREFIX)/lib

local:
	$(BUILD_PY) $(BUILD_ARGS)
	$(RM) $(SL_BIN_NAME)
	ln -s out/$(SL_BIN_NAME) $(SL_BIN_NAME)

hg:
	$(BUILD_PY) $(BUILD_ARGS) sl
	$(RM) $(HG_BIN_NAME)
	ln -s out/$(SL_BIN_NAME) $(HG_BIN_NAME)

sl:
	$(BUILD_PY) $(BUILD_ARGS) sl
	$(RM) $(SL_BIN_NAME)
	ln -s out/$(SL_BIN_NAME) $(SL_BIN_NAME)

build:
	$(BUILD_PY) $(BUILD_ARGS)

getdepsbuild: HGNAME=$(SL_NAME)
getdepsbuild:
	@test -n "$(GETDEPS_BUILD_DIR)" || (echo "GETDEPS_BUILD_DIR is required" >&2; exit 1)
	@test -n "$(GETDEPS_INSTALL_DIR)" || (echo "GETDEPS_INSTALL_DIR is required" >&2; exit 1)
	mkdir -p "$(GETDEPS_BUILD_DIR)/sapling"
	rm -rf out
	ln -sfn "$(GETDEPS_BUILD_DIR)/sapling" out
	HGNAME=$(HGNAME) GETDEPS_BUILD=1 \
		PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
		THRIFT="$(GETDEPS_INSTALL_DIR)/fbthrift/bin/thrift1" \
		$(PYTHON_SYS_EXECUTABLE) ./build.py --getdeps \
		--with-python $(PYTHON_SYS_EXECUTABLE) sl

install-getdeps:
	mkdir -p "$(PREFIX)/bin"
	cp out/$(SL_BIN_NAME) "$(PREFIX)/bin/$(SL_BIN_NAME)"
	cd "$(PREFIX)/bin" && ln -f "$(SL_BIN_NAME)" "$(HG_BIN_NAME)"

# Exclusions for OSS getdeps cli tests.  Newline separated
# test-casecollision.t: flaky on github CI, intermittent FATAL: exception not rethrown
# test-auth-cats.t: internal crypto auth token test
# test-cats.t: internal crpyto token test
# test-check-execute.t: fails on CI, passes locally on ubuntu-22.04
# test-clone-per-repo-config.t: fails in oss, looks like depends on fb hgrc.dynamic
# test-clone-resume.t: fails on CI, passes locally on ubuntu-22.04
# test-commitcloud-sync.t: flaky Last Sync Version on line 812, flips between 17 and 16
# test-config-precedence.t:  output mismatch: DEBUG configloader::hg: spawn ["false"] because * (glob)
# test-copytrace-amend.t: flaky on github CI, intermittent core dump
# test-copytrace-heuristics.t:  flaky on github CI, intermittent core dump
# test-dynamicconfig-unicode.t: output mismatch: cat: .hg/hgrc.dynamic: $ENOENT$
# test-debugrefreshconfig.t: assumes an internal config location
# test-fastlog.t: timesout, maybe due to internal endpoint assumptions
# test-help.t: help is different vs internal build
# test-histedit-fold.t: flaky on github CI, intermittent core dump
# test-include-fail.t: fails on CI, passes locally on ubuntu-22.04
# test-issue586.t: flaky on github CI, intermittent FATAL: exception not rethrown
# test-matcher-lots-of-globs.t: fails on CI, passes locally on ubuntu-22.04 (needs ~40GiB RAM)
# test-network-doctor.t: times out
# test-rust-checkout.t: fails on CI, passes locally on ubuntu-22.04
# test-sampling.t: timeout
# test-smartlog.t: output mismatch
# test-smartlog-interactive.t: smartlog format is different causing output mismatch
# test-smartlog-interactive-highlighting.t: smartlog format is different causing output mismatch
# test-agent-advice.t: depends on Meta-internal agent instructions (fb/ module)
# test-code-format.t: depends on Meta-internal code format (fb/ module)
GETDEPS_TEST_EXCLUSION_LIST := \
  test-agent-advice.t \
  test-auth-cats.t \
  test-biggrep.t \
  test-casecollision.t \
  test-cats.t \
  test-code-format.t \
  test-check-execute.t \
  test-clone-per-repo-config.t \
  test-clone-resume.t \
  test-commitcloud-sync.t \
  test-config-precedence.t \
  test-copytrace-amend.t \
  test-copytrace-heuristics.t \
  test-dynamicconfig-unicode.t \
  test-debugrefreshconfig.t \
  test-fastlog.t \
  test-help.t \
  test-histedit-fold.t \
  test-include-fail.t \
  test-issue586.t \
  test-matcher-lots-of-globs.t \
  test-network-doctor.t \
  test-rust-checkout.t \
  test-sampling.t \
  test-smartlog.t \
  test-smartlog-interactive.t \
  test-smartlog-interactive-highlighting.t \
  test-tweakdefaults-grep.t

ifeq ($(PYTHON_MINOR_VERSION),12)
# output differs for these on 3.12 in ways that are hard to match for 3.10 and 3.12 simultaneously
GETDEPS_TEST_EXCLUSION_LIST := $(GETDEPS_TEST_EXCLUSION_LIST) \
	test-eager-exchange.t
endif

# convert to a sed expression
GETDEPS_TEST_EXCLUSIONS := $(subst $() $(),|,$(GETDEPS_TEST_EXCLUSION_LIST))

.PHONY: test-getdeps
test-getdeps:
    # Remove the .testfailed and .testerrored files so that after this next
	# step they are written clean
	rm -f ./tests/.test*
	# ensure that fbpython is present, as some tests depend on it being on PATH
	if ! which fbpython >/dev/null 2>&1; then \
		FBPYTHON="$(GETDEPS_INSTALL_DIR)/sapling/bin/fbpython"; \
		PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE); \
		printf "#!/bin/sh\nexec \"$$PYTHON_SYS_EXECUTABLE\" \"\$$@\"\n"  > $$FBPYTHON; \
		chmod +x "$$FBPYTHON"; \
	fi;
	# Run tests and retry any failures
	export GETDEPS_BUILD=1; \
	export HGTEST_HG=$(GETDEPS_INSTALL_DIR)/sapling/bin/$(HG_BIN_NAME); \
	cd tests && export PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE); \
	for try in $$(seq 0 $(GETDEPS_TEST_RETRY)); do \
		RERUN_ARG=""; \
		GETDEPS_TEST_FILTER=$(GETDEPS_TEST_FILTER); \
		if [ $$try -gt 0 ]; then \
			GETDEPS_TEST_FILTER=$$(cat .testfailed .testerrored | sort -u | grep -v '^$$'); \
			if [ -z "$$GETDEPS_TEST_FILTER" ]; then "Echo no tests found for rerun on try $$try"; exit 2; fi; \
			echo "Rerunning: $$GETDEPS_TEST_FILTER on try $$try" 1>&2; \
			rm -f .testfailed .testerrored; \
		elif [ -z "$$GETDEPS_TEST_FILTER" ]; then \
		    GETDEPS_TEST_FILTER=$$(echo *.t | sed -Ee 's/($(GETDEPS_TEST_EXCLUSIONS))//g'); \
		fi; \
		$$PYTHON_SYS_EXECUTABLE run-tests.py -j $(JOBS) --getdeps-build --with-hg="$(PREFIX)/bin/$(HGNAME)" $$GETDEPS_TEST_FILTER; \
		status=$$?; \
		if [ $$status = 0 ]; then echo "passed on try $$try"; exit 0; fi; \
	done; \
	exit $$status

check: tests

.PHONY: tests
tests:
	cd tests && PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
	    $(PYTHON_SYS_EXECUTABLE) run-tests.py

buck:
	cp $(shell buck2 build $(BUCK_BUILD_MODE) fbcode//eden/scm:hg --show-full-simple-output) $(HG_BIN_NAME)

buck-isl:
	cp $(shell buck2 build $(BUCK_BUILD_MODE) fbcode//eden/addons:isl-tar --show-full-simple-output) $(ISL_NAME)

# Packaging targets

.PHONY: help all local build install-getdeps getdepsbuild hg sl buck

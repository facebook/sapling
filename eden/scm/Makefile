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

ifeq ($(RUST_DEBUG),1)
RUST_FLAG = --debug
endif

$(eval HGROOT := $(shell pwd))
HGPYTHONS ?= $(HGROOT)/build/pythons
PURE=
PYFILES:=$(shell find mercurial ext -name '*.py' 2>/dev/null)
DOCFILES=sapling/help/*.txt
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
	@echo '  all          - build program'
	@echo '  install      - install program and man pages to $$PREFIX ($(PREFIX))'
	@echo '  install-home - install with setup.py install --home=$$HOME ($(HOME))'
	@echo '  local        - build for inplace usage'
	@echo '  tests        - run all tests in the automatic test suite'
	@echo '  test-foo     - run only specified tests (e.g. test-merge1.t)'
	@echo '  dist         - run all tests and create a source tarball in dist/'
	@echo '  clean        - remove files created by other targets'
	@echo '                 (except installed files or dist source tarball)'
	@echo '  update-pot   - update i18n/hg.pot'
	@echo
	@echo 'Example for a system-wide installation under /usr/local:'
	@echo '  make all && su -c "make install" && hg version'
	@echo
	@echo 'Example for a local installation (usable in this directory):'
	@echo '  make local && ./hg version'

all: build

oss: OSS=true
oss: HGNAME=$(SL_NAME)
oss: HG_BIN_NAME=$(SL_BIN_NAME)
oss: local

install-oss: oss
	mkdir -p $(DESTDIR)/$(PREFIX)/bin
	cp $(SL_NAME) $(DESTDIR)/$(PREFIX)/bin
	mkdir -p $(DESTDIR)/$(PREFIX)/lib
	cp isl-dist.tar.xz $(DESTDIR)/$(PREFIX)/lib

local:
	SAPLING_OSS_BUILD=$(OSS) HGNAME=$(HGNAME) \
	  $(PYTHON_SYS_EXECUTABLE) setup.py $(PURE) \
	  build_interactive_smartlog \
	  build_rust_ext -i -l $(RUST_FLAG) \
	  build_mo
ifneq ($(OS),Windows_NT)
	$(RM) $(HGNAME)
endif
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HG_BIN_NAME) $(HG_BIN_NAME)

hg:
	SAPLING_SKIP_OTHER_RUST_BINARIES=true $(PYTHON_SYS_EXECUTABLE) setup.py $(PURE) \
	  build_rust_ext -i -l $(RUST_FLAG) \
	  build_mo
ifneq ($(OS),Windows_NT)
	$(RM) $(HGNAME)
endif
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HG_BIN_NAME) $(HG_BIN_NAME)

build:
	$(PYTHON) setup.py $(PURE) build $(COMPILERFLAG)

deb:
	./packaging/debian/build_deb.sh

rpm:
	rpmbuild \
		--define "sapling_root `pwd`" \
		--define "version $(VERSION)" \
		-bb packaging/rpm/sapling.spec

wheel:
	FORCE_SETUPTOOLS=1 $(PYTHON) setup.py $(PURE) bdist_wheel $(COMPILERFLAG)

getdepsbuild: HGNAME=$(SL_NAME)
getdepsbuild:
	mkdir -p $(GETDEPS_BUILD_DIR)/sapling
	ln -sfn $(GETDEPS_BUILD_DIR)/sapling build
	HGNAME=$(HGNAME) GETDEPS_BUILD=1 \
		PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
		THRIFT="$(GETDEPS_INSTALL_DIR)/fbthrift/bin/thrift1" \
		$(PYTHON_SYS_EXECUTABLE) setup.py \
		$(PURE) build $(COMPILERFLAG)

cleanbutpackages:
	-$(PYTHON3) setup.py clean --all # ignore errors from this command
	find contrib doc i18n sapling tests \
		\( -name '*.py[cdo]' -o -name '*.so' \) -exec rm -f '{}' ';'
	rm -f MANIFEST MANIFEST.in sapling/ext/__index__.py tests/*.err
	rm -f sapling/__modulepolicy__.py
	if test -d .hg; then rm -f sapling/__version__.py; fi
	rm -rf build/*
	rm -rf build sapling/locale
	rm -f Cargo.lock
ifeq ($(OS),Windows_NT)
	$(RM) -r hg-python $(HGNAME).exe python27.dll
else
	$(RM) $(HGNAME)
endif

clean: cleanbutpackages
	rm -rf packages

install: build
	$(PYTHON) setup.py $(PURE) install --root="$(DESTDIR)/" --prefix="$(PREFIX)" --force

install-home: build
	$(PYTHON) setup.py $(PURE) install --home="$(HOME)" --prefix="" --force

install-getdeps:
	PYTHON_SYS_EXECUTABLE=$(PYTHON_SYS_EXECUTABLE) \
	    GETDEPS_BUILD=1 $(PYTHON_SYS_EXECUTABLE) \
		setup.py $(PURE) install --skip-build --prefix="$(PREFIX)" --install-scripts="$(PREFIX)/bin"  --install-lib="$(PREFIX)/bin" --force && \
		cd "$(PREFIX)/bin" && ln -f "$(SL_BIN_NAME)" "$(HG_BIN_NAME)"

# Exclusions for OSS getdeps cli tests.  Newline separated
# test-casecollision.t: flaky on github CI, intermittent FATAL: exception not rethrown
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
GETDEPS_TEST_EXCLUSION_LIST := test-casecollision.t \
	test-cats.t \
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
	test-smartlog-interactive-highlighting.t

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

update-pot: i18n/hg.pot

i18n/hg.pot: $(PYFILES) $(DOCFILES) i18n/posplit i18n/hggettext
	$(PYTHON) i18n/hggettext sapling/commands.py \
	  sapling/ext/*.py sapling/ext/*/__init__.py \
	  sapling/fileset.py sapling/revset.py \
	  sapling/templatefilters.py sapling/templatekw.py \
	  sapling/templater.py \
	  sapling/filemerge.py \
	  sapling/util.py \
	  $(DOCFILES) > i18n/hg.pot.tmp
        # All strings marked for translation in Mercurial contain
        # ASCII characters only. But some files contain string
        # literals like this '\037\213'. xgettext thinks it has to
        # parse them even though they are not marked for translation.
        # Extracting with an explicit encoding of ISO-8859-1 will make
        # xgettext "parse" and ignore them.
	echo $(PYFILES) | xargs \
	  xgettext --package-name "Mercurial" \
	  --msgid-bugs-address "<mercurial-devel@mercurial-scm.org>" \
	  --copyright-holder "Olivia Mackall <olivia@selenic.com> and others" \
	  --from-code ISO-8859-1 --join --sort-by-file --add-comments=i18n: \
	  --keyword=_n:1,2 -d hg -p i18n -o hg.pot.tmp
	$(PYTHON) i18n/posplit i18n/hg.pot.tmp
        # The target file is not created before the last step. So it never is in
        # an intermediate state.
	mv -f i18n/hg.pot.tmp i18n/hg.pot

%.po: i18n/hg.pot
        # work on a temporary copy for never having a half completed target
	cp $@ $@.tmp
	msgmerge --no-location --update $@.tmp $^
	mv -f $@.tmp $@

buck:
	cp $(shell buck2 build $(BUCK_BUILD_MODE) fbcode//eden/scm:hg --show-full-simple-output) $(HG_BIN_NAME)

buck-isl:
	cp $(shell buck2 build $(BUCK_BUILD_MODE) fbcode//eden/addons:isl-tar --show-full-simple-output) $(ISL_NAME)

# Packaging targets

.PHONY: help all local build cleanbutpackages clean install install-home install-getdeps getdepsbuild deb hg buck

# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# If you want to change PREFIX, do not just edit it below. The changed
# value wont get passed on to recursive make calls. You should instead
# override the variable on the command like:
#
# % make PREFIX=/opt/ install

export PREFIX=/usr/local

PYTHON ?= $(shell python3 contrib/pick_python.py)
export PYTHON_SYS_EXECUTABLE=$(PYTHON)

ifeq ($(OS),Windows_NT)
  BUCK_DEV_BUILD_MODE := @fbcode//mode/win
else
  UNAME_S := $(shell uname -s)
  ifeq ($(UNAME_S),Darwin)
    BUCK_DEV_BUILD_MODE := @fbcode//mode/mac
  endif
endif

PYTHON_MINOR_VERSION=$(shell $(PYTHON) -c "import sys; print(sys.version_info.minor)")

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
SL_NAME = sl

# Set this to e.g. "mingw32" to use a non-default compiler.
COMPILER=

COMPILERFLAG_tmp_ =
COMPILERFLAG_tmp_${COMPILER} ?= -c $(COMPILER)
COMPILERFLAG=${COMPILERFLAG_tmp_${COMPILER}}

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
oss: local

install-oss: oss
	mkdir -p $(DESTDIR)/$(PREFIX)/bin
	cp $(SL_NAME) $(DESTDIR)/$(PREFIX)/bin
	mkdir -p $(DESTDIR)/$(PREFIX)/lib
	cp isl-dist.tar.xz $(DESTDIR)/$(PREFIX)/lib

local:
	SAPLING_OSS_BUILD=$(OSS) HGNAME=$(HGNAME) \
	  $(PYTHON) setup.py $(PURE) \
	  build_interactive_smartlog \
	  build_clib $(COMPILERFLAG) \
	  build_rust_ext -i -l $(RUST_FLAG) \
	  build_mo
ifeq ($(OS),Windows_NT)
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HGNAME).exe $(HGNAME).exe
else
	$(RM) $(HGNAME)
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HGNAME) $(HGNAME)
endif

hg:
	SAPLING_SKIP_OTHER_RUST_BINARIES=true $(PYTHON) setup.py $(PURE) \
	  build_clib $(COMPILERFLAG) \
	  build_rust_ext -i -l $(RUST_FLAG) \
	  build_mo
ifeq ($(OS),Windows_NT)
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HGNAME).exe $(HGNAME).exe
else
	$(RM) $(HGNAME)
	cp build/scripts-3.$(PYTHON_MINOR_VERSION)/$(HGNAME) $(HGNAME)
endif

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

getdepsbuild:
	mkdir -p $(GETDEPS_BUILD_DIR)/eden_scm
	ln -sfn $(GETDEPS_BUILD_DIR)/eden_scm build
	GETDEPS_BUILD=1 \
		THRIFT="$(GETDEPS_INSTALL_DIR)/fbthrift/bin/thrift1" \
		$(PYTHON) setup.py \
		$(PURE) build $(COMPILERFLAG)

cleanbutpackages:
	-$(PYTHON) setup.py clean --all # ignore errors from this command
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

install-getdeps: getdepsbuild
	    GETDEPS_BUILD=1 $(PYTHON) \
		setup.py $(PURE) install --root="$(DESTDIR)/" --prefix="$(PREFIX)" --install-lib="$(PREFIX)/bin" --force

check: tests

.PHONY: tests
tests:
	cd tests && \
	    $(PYTHON) run-tests.py

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
	cp $(shell buck2 build $(BUCK_DEV_BUILD_MODE) fbcode//eden/scm:hg --show-full-simple-output) $(HGNAME)

# Packaging targets

.PHONY: help all local build cleanbutpackages clean install install-home install-getdeps getdepsbuild deb hg buck

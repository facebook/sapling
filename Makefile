# If you want to change PREFIX, do not just edit it below. The changed
# value wont get passed on to recursive make calls. You should instead
# override the variable on the command like:
#
# % make PREFIX=/opt/ install

export PREFIX=/usr/local

PYTHON := python

$(eval HGROOT := $(shell pwd))
HGPYTHONS ?= $(HGROOT)/build/pythons
PURE=
PYFILES:=$(shell find mercurial hgext -name '*.py' 2>/dev/null)
DOCFILES=edenscm/mercurial/help/*.txt
export LANGUAGE=C
export LC_ALL=C
TESTFLAGS ?= $(shell echo $$HGTESTFLAGS)
OSXVERSIONFLAGS ?= $(shell echo $$OSXVERSIONFLAGS)

HGNAME ?= hg

# Set this to e.g. "mingw32" to use a non-default compiler.
COMPILER=

COMPILERFLAG_tmp_ =
COMPILERFLAG_tmp_${COMPILER} ?= -c $(COMPILER)
COMPILERFLAG=${COMPILERFLAG_tmp_${COMPILER}}

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

local:
	$(PYTHON) setup.py $(PURE) \
	  build_py -c -d . \
	  build_clib $(COMPILERFLAG) \
	  build_ext $(COMPILERFLAG) -i \
	  build_rust_ext -i -l $(RFLAG)\
	  build_pyzip -i \
	  build_mo
ifeq ($(OS),Windows_NT)
	cp build/scripts-2.7/$(HGNAME).exe $(HGNAME).exe
else
	$(RM) $(HGNAME)
	cp build/scripts-2.7/$(HGNAME) $(HGNAME)
endif

build:
	$(PYTHON) setup.py $(PURE) build $(COMPILERFLAG)

wheel:
	FORCE_SETUPTOOLS=1 $(PYTHON) setup.py $(PURE) bdist_wheel $(COMPILERFLAG)

cleanbutpackages:
	-$(PYTHON) setup.py clean --all # ignore errors from this command
	find contrib doc i18n edenscm tests \
		\( -name '*.py[cdo]' -o -name '*.so' \) -exec rm -f '{}' ';'
	rm -f MANIFEST MANIFEST.in edenscm/hgext/__index__.py tests/*.err
	rm -f edenscm/mercurial/__modulepolicy__.py
	if test -d .hg; then rm -f edenscm/mercurial/__version__.py; fi
	rm -rf build/*
	rm -rf build edenscm/mercurial/locale
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

check: tests

update-pot: i18n/hg.pot

i18n/hg.pot: $(PYFILES) $(DOCFILES) i18n/posplit i18n/hggettext
	$(PYTHON) i18n/hggettext edenscm/mercurial/commands.py \
	  edenscm/hgext/*.py edenscm/hgext/*/__init__.py \
	  edenscm/mercurial/fileset.py edenscm/mercurial/revset.py \
	  edenscm/mercurial/templatefilters.py edenscm/mercurial/templatekw.py \
	  edenscm/mercurial/templater.py \
	  edenscm/mercurial/filemerge.py \
	  edenscm/mercurial/hgweb/webcommands.py \
	  edenscm/mercurial/util.py \
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
	  --copyright-holder "Matt Mackall <mpm@selenic.com> and others" \
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

# Packaging targets

.PHONY: help all local build cleanbutpackages clean install install-home

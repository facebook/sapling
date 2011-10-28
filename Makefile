# If you want to change PREFIX, do not just edit it below. The changed
# value wont get passed on to recursive make calls. You should instead
# override the variable on the command like:
#
# % make PREFIX=/opt/ install

PREFIX=/usr/local
export PREFIX
PYTHON=python
PURE=
PYFILES:=$(shell find mercurial hgext doc -name '*.py')
DOCFILES=mercurial/help/*.txt

help:
	@echo 'Commonly used make targets:'
	@echo '  all          - build program and documentation'
	@echo '  install      - install program and man pages to PREFIX ($(PREFIX))'
	@echo '  install-home - install with setup.py install --home=HOME ($(HOME))'
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

all: build doc

local:
	$(PYTHON) setup.py $(PURE) build_py -c -d . build_ext -i build_mo
	$(PYTHON) hg version

build:
	$(PYTHON) setup.py $(PURE) build

doc:
	$(MAKE) -C doc

clean:
	-$(PYTHON) setup.py clean --all # ignore errors from this command
	find . \( -name '*.py[cdo]' -o -name '*.so' \) -exec rm -f '{}' ';'
	rm -f $(addprefix mercurial/,$(notdir $(wildcard mercurial/pure/*.py)))
	rm -f MANIFEST MANIFEST.in tests/*.err
	rm -rf build mercurial/locale
	$(MAKE) -C doc clean

install: install-bin install-doc

install-bin: build
	$(PYTHON) setup.py $(PURE) install --root="$(DESTDIR)/" --prefix="$(PREFIX)" --force

install-doc: doc
	cd doc && $(MAKE) $(MFLAGS) install

install-home: install-home-bin install-home-doc

install-home-bin: build
	$(PYTHON) setup.py $(PURE) install --home="$(HOME)" --force

install-home-doc: doc
	cd doc && $(MAKE) $(MFLAGS) PREFIX="$(HOME)" install

MANIFEST-doc:
	$(MAKE) -C doc MANIFEST

MANIFEST.in: MANIFEST-doc
	hg manifest | sed -e 's/^/include /' > MANIFEST.in
	echo include mercurial/__version__.py >> MANIFEST.in
	sed -e 's/^/include /' < doc/MANIFEST >> MANIFEST.in

dist:	tests dist-notests

dist-notests:	doc MANIFEST.in
	TAR_OPTIONS="--owner=root --group=root --mode=u+w,go-w,a+rX-s" $(PYTHON) setup.py -q sdist

check: tests

tests:
	cd tests && $(PYTHON) run-tests.py $(TESTFLAGS)

test-%:
	cd tests && $(PYTHON) run-tests.py $(TESTFLAGS) $@

update-pot: i18n/hg.pot

i18n/hg.pot: $(PYFILES) $(DOCFILES)
	$(PYTHON) i18n/hggettext mercurial/commands.py \
	  hgext/*.py hgext/*/__init__.py mercurial/fileset.py mercurial/revset.py \
	  mercurial/templatefilters.py mercurial/templatekw.py \
	  $(DOCFILES) > i18n/hg.pot
        # All strings marked for translation in Mercurial contain
        # ASCII characters only. But some files contain string
        # literals like this '\037\213'. xgettext thinks it has to
        # parse them even though they are not marked for translation.
        # Extracting with an explicit encoding of ISO-8859-1 will make
        # xgettext "parse" and ignore them.
	echo $(PYFILES) | xargs \
	  xgettext --package-name "Mercurial" \
	  --msgid-bugs-address "<mercurial-devel@selenic.com>" \
	  --copyright-holder "Matt Mackall <mpm@selenic.com> and others" \
	  --from-code ISO-8859-1 --join --sort-by-file --add-comments=i18n: \
	  -d hg -p i18n -o hg.pot
	$(PYTHON) i18n/posplit i18n/hg.pot

%.po: i18n/hg.pot
	msgmerge --no-location --update $@ $^

.PHONY: help all local build doc clean install install-bin install-doc \
	install-home install-home-bin install-home-doc dist dist-notests tests \
	update-pot

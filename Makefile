PYTHON=python
TESTFLAGS ?= $(shell echo $$HGTESTFLAGS)

help:
	@echo 'Commonly used make targets:'
	@echo '  tests              - run all tests in the automatic test suite'
	@echo '  all-version-tests  - run all tests against many hg versions'
	@echo '  tests-%s           - run all tests in the specified hg version'

all: help

tests:
	cd tests && $(PYTHON) run-tests.py --with-hg=`which hg` $(TESTFLAGS)

test-%:
	python -m doctest hggit/hg2git.py
	cd tests && $(PYTHON) run-tests.py --with-hg=`which hg` $(TESTFLAGS) $@

tests-%:
	@echo "Path to crew repo is $(CREW) - set this with CREW= if needed."
	hg -R $(CREW) checkout $$(echo $@ | sed s/tests-//) && \
	(cd $(CREW) ; $(MAKE) clean ) && \
	cd tests && $(PYTHON) $(CREW)/tests/run-tests.py $(TESTFLAGS)

# This is intended to be the authoritative list of Hg versions that this
# extension is tested with.  Versions prior to the version that ships in the
# latest Ubuntu LTS release (2.8.2 for 14.04 LTS) may be dropped if they
# interfere with new development.  The latest released minor version should be
# listed for each major version; earlier minor versions are not needed.
# Mercurial 3.4 had a core bug that caused a harmless test failure -- 3.4.1
# fixes that bug.

all-version-tests: tests-2.8.2 tests-3.0.1 tests-3.1 tests-3.2.2 tests-3.3 tests-3.4.1 tests-3.5 tests-3.6 tests-@

.PHONY: tests all-version-tests

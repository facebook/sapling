PYTHON=python

help:
	@echo 'Commonly used make targets:'
	@echo '  tests        - run all tests in the automatic test suite'

all: tests

tests:
	cd tests && $(PYTHON) run-tests.py --with-hg=`which hg` $(TESTFLAGS)

test-%:
	cd tests && $(PYTHON) run-tests.py --with-hg=`which hg` $(TESTFLAGS) $@
.PHONY: help all local build doc clean install install-bin install-doc \
	install-home install-home-bin install-home-doc dist dist-notests tests \
	update-pot

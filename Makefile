# Makefile for testing hgsubversion

PYTHON=python

.PHONY: all check check-demandimport check-subvertpy check-swig

all:
	@echo "Use the following commands to build and install hgsubversion:"
	@echo
	@echo "$$ cd $(PWD)"
	@echo "$$ $(PYTHON) ./setup.py install"
	@echo
	@exit 1

check: check-demandimport check-subvertpy check-swig

check-demandimport:
	# verify that hgsubversion loads properly without bindings, but fails
	# when actually used
	! LC_ALL=C HGSUBVERSION_BINDINGS=none HGRCPATH=/dev/null \
	  hg --config extensions.hgsubversion=./hgsubversion \
	  version 2>&1 \
	  | egrep '(^abort:|failed to import extension)'
	LC_ALL=C HGSUBVERSION_BINDINGS=none HGRCPATH=/dev/null \
	  hg --config extensions.hgsubversion=./hgsubversion \
	  version --svn 2>&1 \
	  | egrep '(^abort:|failed to import extension)'

check-subvertpy:
	$(PYTHON) tests/run.py --all --bindings=subvertpy

check-swig:
	$(PYTHON) tests/run.py --all --bindings=swig

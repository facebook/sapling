PREFIX=/usr/local
export PREFIX
PYTHON=python

all: local build doc

local:
	$(PYTHON) setup.py build_ext -i

build:
	$(PYTHON) setup.py build

doc:
	$(MAKE) -C doc

clean:
	-$(PYTHON) setup.py clean --all # ignore errors of this command
	find . -name '*.py[co]' -exec rm -f '{}' ';'
	$(MAKE) -C doc clean

install: all
	$(PYTHON) setup.py install --prefix="$(PREFIX)" --force
	cd doc && $(MAKE) $(MFLAGS) install

install-home: all
	$(PYTHON) setup.py install --home="$(HOME)" --force
	cd doc && $(MAKE) $(MFLAGS) PREFIX="$(HOME)" install

dist:	tests dist-notests

dist-notests:	doc
	TAR_OPTIONS="--owner=root --group=root --mode=u+w,go-w,a+rX-s" $(PYTHON) setup.py sdist --force-manifest

tests:
	cd tests && $(PYTHON) run-tests.py

test-%:
	cd tests && $(PYTHON) run-tests.py $@


.PHONY: all local build doc clean install install-home dist dist-notests tests


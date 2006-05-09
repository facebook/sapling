PREFIX=/usr/local
export PREFIX
PYTHON=python

all:
	$(PYTHON) setup.py build_ext -i

install: all
	$(PYTHON) setup.py install --home="$(PREFIX)"
	cd doc && $(MAKE) $(MFLAGS) install

clean:
	-$(PYTHON) setup.py clean --all # ignore errors of this command
	find . -name '*.py[co]' -exec rm -f '{}' ';'
	$(MAKE) -C doc clean

dist:	tests dist-notests

dist-notests:	doc
	TAR_OPTIONS="--owner=root --group=root --mode=u+w,go-w,a+rX-s" $(PYTHON) setup.py sdist --force-manifest

tests:
	cd tests && $(PYTHON) run-tests.py

test-%:
	cd tests && $(PYTHON) run-tests.py $@

doc:
	$(MAKE) -C doc


.PHONY: all clean dist dist-notests tests doc


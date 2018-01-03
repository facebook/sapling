PYTHON=python

.PHONY: tests

PREFIX=/usr/local

help:
	@echo 'Commonly used make targets:'
	@echo '  local          - build for inplace use'
	@echo '  install        - install program and man pages to PREFIX ($(PREFIX))'
	@echo '  clean          - remove files created by other targets'
	@echo '                   (except installed files or dist source tarball)'

local:
	$(PYTHON) setup.py \
	  build_py -c -d . \
	  build_clib \
	  build_ext -i \
	  build_rust_ext -i

install:
	$(PYTHON) setup.py $(PURE) install --prefix="$(PREFIX)" --force

clean:
	-$(PYTHON) setup.py clean --all clean_ext # ignore errors from this command

deb:
	contrib/builddeb

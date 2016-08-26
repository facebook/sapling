PYTHON=python

.PHONY: tests

help:
	@echo 'Commonly used make targets:'
	@echo '  local          - build for inplace use'
	@echo '  install        - install program'
	@echo '  clean          - remove files created by other targets'
	@echo '                   (except installed files or dist source tarball)'

local:
	$(PYTHON) setup.py \
	  build_py -c -d . \
	  build_ext -i

install:
	$(PYTHON) setup.py $(PURE) install --force

clean:
	-$(PYTHON) setup.py clean --all # ignore errors from this command
	find . \( -name '*.py[cdo]' -o -name '*.so' \) -exec rm -f '{}' ';'

deb:
	contrib/builddeb

# If you want to change PREFIX, do not just edit it below. The changed
# value wont get passed on to recursive make calls. You should instead
# override the variable on the command like:
#
# % make PREFIX=/opt/ install

export PREFIX=/usr/local
PYTHON=python
$(eval HGROOT := $(shell pwd))
HGPYTHONS ?= $(HGROOT)/build/pythons
PURE=
PYFILES:=$(shell find mercurial hgext doc -name '*.py')
DOCFILES=mercurial/help/*.txt
export LANGUAGE=C
export LC_ALL=C
TESTFLAGS ?= $(shell echo $$HGTESTFLAGS)
OSXVERSIONFLAGS ?= $(shell echo $$OSXVERSIONFLAGS)

# Set this to e.g. "mingw32" to use a non-default compiler.
COMPILER=

COMPILERFLAG_tmp_ =
COMPILERFLAG_tmp_${COMPILER} ?= -c $(COMPILER)
COMPILERFLAG=${COMPILERFLAG_tmp_${COMPILER}}

help:
	@echo 'Commonly used make targets:'
	@echo '  all          - build program and documentation'
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

all: build doc

local:
	$(PYTHON) setup.py $(PURE) \
	  build_py -c -d . \
	  build_ext $(COMPILERFLAG) -i \
	  build_hgexe $(COMPILERFLAG) -i \
	  build_mo
	env HGRCPATH= $(PYTHON) hg version

build:
	$(PYTHON) setup.py $(PURE) build $(COMPILERFLAG)

wheel:
	FORCE_SETUPTOOLS=1 $(PYTHON) setup.py $(PURE) bdist_wheel $(COMPILERFLAG)

doc:
	$(MAKE) -C doc

cleanbutpackages:
	-$(PYTHON) setup.py clean --all # ignore errors from this command
	find contrib doc hgext hgext3rd i18n mercurial tests \
		\( -name '*.py[cdo]' -o -name '*.so' \) -exec rm -f '{}' ';'
	rm -f MANIFEST MANIFEST.in hgext/__index__.py tests/*.err
	rm -f mercurial/__modulepolicy__.py
	if test -d .hg; then rm -f mercurial/__version__.py; fi
	rm -rf build mercurial/locale
	$(MAKE) -C doc clean
	$(MAKE) -C contrib/chg distclean

clean: cleanbutpackages
	rm -rf packages

install: install-bin install-doc

install-bin: build
	$(PYTHON) setup.py $(PURE) install --root="$(DESTDIR)/" --prefix="$(PREFIX)" --force

install-doc: doc
	cd doc && $(MAKE) $(MFLAGS) install

install-home: install-home-bin install-home-doc

install-home-bin: build
	$(PYTHON) setup.py $(PURE) install --home="$(HOME)" --prefix="" --force

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

testpy-%:
	@echo Looking for Python $* in $(HGPYTHONS)
	[ -e $(HGPYTHONS)/$*/bin/python ] || ( \
	cd $$(mktemp --directory --tmpdir) && \
        $(MAKE) -f $(HGROOT)/contrib/Makefile.python PYTHONVER=$* PREFIX=$(HGPYTHONS)/$* python )
	cd tests && $(HGPYTHONS)/$*/bin/python run-tests.py $(TESTFLAGS)

check-code:
	hg manifest | xargs python contrib/check-code.py

update-pot: i18n/hg.pot

i18n/hg.pot: $(PYFILES) $(DOCFILES) i18n/posplit i18n/hggettext
	$(PYTHON) i18n/hggettext mercurial/commands.py \
	  hgext/*.py hgext/*/__init__.py \
	  mercurial/fileset.py mercurial/revset.py \
	  mercurial/templatefilters.py mercurial/templatekw.py \
	  mercurial/templater.py \
	  mercurial/filemerge.py \
	  mercurial/hgweb/webcommands.py \
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
	  -d hg -p i18n -o hg.pot.tmp
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

osx:
	rm -rf build/mercurial
	/usr/bin/python2.7 setup.py install --optimize=1 \
	  --root=build/mercurial/ --prefix=/usr/local/ \
	  --install-lib=/Library/Python/2.7/site-packages/
	make -C doc all install DESTDIR="$(PWD)/build/mercurial/"
        # Place a bogon .DS_Store file in the target dir so we can be
        # sure it doesn't get included in the final package.
	touch build/mercurial/.DS_Store
        # install zsh completions - this location appears to be
        # searched by default as of macOS Sierra.
	install -d build/mercurial/usr/local/share/zsh/site-functions/
	install -m 0644 contrib/zsh_completion build/mercurial/usr/local/share/zsh/site-functions/_hg
        # install bash completions - there doesn't appear to be a
        # place that's searched by default for bash, so we'll follow
        # the lead of Apple's git install and just put it in a
        # location of our own.
	install -d build/mercurial/usr/local/hg/contrib/
	install -m 0644 contrib/bash_completion build/mercurial/usr/local/hg/contrib/hg-completion.bash
	make -C contrib/chg \
	  HGPATH=/usr/local/bin/hg \
	  PYTHON=/usr/bin/python2.7 \
	  HG=/usr/local/bin/hg \
	  HGEXTDIR=/Library/Python/2.7/site-packages/hgext \
	  DESTDIR=../../build/mercurial \
	  PREFIX=/usr/local \
	  clean install
	mkdir -p $${OUTPUTDIR:-dist}
	HGVER=$(shell python contrib/genosxversion.py $(OSXVERSIONFLAGS) build/mercurial/Library/Python/2.7/site-packages/mercurial/__version__.py ) && \
	OSXVER=$$(sw_vers -productVersion | cut -d. -f1,2) && \
	pkgbuild --filter \\.DS_Store --root build/mercurial/ \
	  --identifier org.mercurial-scm.mercurial \
	  --version "$${HGVER}" \
	  build/mercurial.pkg && \
	productbuild --distribution contrib/macosx/distribution.xml \
	  --package-path build/ \
	  --version "$${HGVER}" \
	  --resources contrib/macosx/ \
	  "$${OUTPUTDIR:-dist/}"/Mercurial-"$${HGVER}"-macosx"$${OSXVER}".pkg

deb:
	contrib/builddeb

ppa:
	contrib/builddeb --source-only

contrib/docker/debian-%: contrib/docker/debian.template
	sed "s/__CODENAME__/$*/" $< > $@

docker-debian-jessie: contrib/docker/debian-jessie
	mkdir -p packages/debian-jessie
	contrib/dockerdeb debian jessie

docker-debian-stretch: contrib/docker/debian-stretch
	mkdir -p packages/debian-stretch
	contrib/dockerdeb debian stretch

contrib/docker/ubuntu-%: contrib/docker/ubuntu.template
	sed "s/__CODENAME__/$*/" $< > $@

docker-ubuntu-trusty: contrib/docker/ubuntu-trusty
	contrib/dockerdeb ubuntu trusty

docker-ubuntu-trusty-ppa: contrib/docker/ubuntu-trusty
	contrib/dockerdeb ubuntu trusty --source-only

docker-ubuntu-xenial: contrib/docker/ubuntu-xenial
	contrib/dockerdeb ubuntu xenial

docker-ubuntu-xenial-ppa: contrib/docker/ubuntu-xenial
	contrib/dockerdeb ubuntu xenial --source-only

docker-ubuntu-yakkety: contrib/docker/ubuntu-yakkety
	contrib/dockerdeb ubuntu yakkety

docker-ubuntu-yakkety-ppa: contrib/docker/ubuntu-yakkety
	contrib/dockerdeb ubuntu yakkety --source-only

docker-ubuntu-zesty: contrib/docker/ubuntu-zesty
	contrib/dockerdeb ubuntu zesty

docker-ubuntu-zesty-ppa: contrib/docker/ubuntu-zesty
	contrib/dockerdeb ubuntu zesty --source-only

fedora20:
	mkdir -p packages/fedora20
	contrib/buildrpm
	cp rpmbuild/RPMS/*/* packages/fedora20
	cp rpmbuild/SRPMS/* packages/fedora20
	rm -rf rpmbuild

docker-fedora20:
	mkdir -p packages/fedora20
	contrib/dockerrpm fedora20

fedora21:
	mkdir -p packages/fedora21
	contrib/buildrpm
	cp rpmbuild/RPMS/*/* packages/fedora21
	cp rpmbuild/SRPMS/* packages/fedora21
	rm -rf rpmbuild

docker-fedora21:
	mkdir -p packages/fedora21
	contrib/dockerrpm fedora21

centos5:
	mkdir -p packages/centos5
	contrib/buildrpm --withpython
	cp rpmbuild/RPMS/*/* packages/centos5
	cp rpmbuild/SRPMS/* packages/centos5

docker-centos5:
	mkdir -p packages/centos5
	contrib/dockerrpm centos5 --withpython

centos6:
	mkdir -p packages/centos6
	contrib/buildrpm --withpython
	cp rpmbuild/RPMS/*/* packages/centos6
	cp rpmbuild/SRPMS/* packages/centos6

docker-centos6:
	mkdir -p packages/centos6
	contrib/dockerrpm centos6 --withpython

centos7:
	mkdir -p packages/centos7
	contrib/buildrpm
	cp rpmbuild/RPMS/*/* packages/centos7
	cp rpmbuild/SRPMS/* packages/centos7

docker-centos7:
	mkdir -p packages/centos7
	contrib/dockerrpm centos7

linux-wheels: linux-wheels-x86_64 linux-wheels-i686

linux-wheels-x86_64:
	docker run -e "HGTEST_JOBS=$(shell nproc)" --rm -ti -v `pwd`:/src quay.io/pypa/manylinux1_x86_64 /src/contrib/build-linux-wheels.sh

linux-wheels-i686:
	docker run -e "HGTEST_JOBS=$(shell nproc)" --rm -ti -v `pwd`:/src quay.io/pypa/manylinux1_i686 linux32 /src/contrib/build-linux-wheels.sh

.PHONY: help all local build doc cleanbutpackages clean install install-bin \
	install-doc install-home install-home-bin install-home-doc \
	dist dist-notests check tests check-code update-pot \
	osx deb ppa docker-debian-jessie docker-debian-stretch \
	docker-ubuntu-trusty docker-ubuntu-trusty-ppa \
	docker-ubuntu-xenial docker-ubuntu-xenial-ppa \
	docker-ubuntu-yakkety docker-ubuntu-yakkety-ppa \
	docker-ubuntu-zesty docker-ubuntu-zesty-ppa \
	fedora20 docker-fedora20 fedora21 docker-fedora21 \
	centos5 docker-centos5 centos6 docker-centos6 centos7 docker-centos7 \
	linux-wheels

SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
GENDOC=gendoc.py ../mercurial/commands.py ../mercurial/help.py ../help/*.txt
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
RST2HTML=$(shell which rst2html 2> /dev/null || echo rst2html.py)

export LC_ALL=C

all: man html

man: $(MAN)

html: $(HTML)

hg.1.txt: hg.1.gendoc.txt
	touch hg.1.txt

hg.1.gendoc.txt: $(GENDOC)
	${PYTHON} gendoc.py > $@.tmp
	mv $@.tmp $@

%: %.txt common.txt
	$(PYTHON) rst2man.py --halt warning \
	  --strip-elements-with-class htmlonly $*.txt $*

%.html: %.txt common.txt
	$(RST2HTML) --halt warning \
	  --link-stylesheet --stylesheet-path style.css $*.txt $*.html

MANIFEST: man html
# tracked files are already in the main MANIFEST
	$(RM) $@
	for i in $(MAN) $(HTML) hg.1.gendoc.txt; do \
	  echo "doc/$$i" >> $@ ; \
	done

install: man
	for i in $(MAN) ; do \
	  subdir=`echo $$i | sed -n 's/^.*\.\([0-9]\)$$/man\1/p'` ; \
	  mkdir -p $(DESTDIR)$(MANDIR)/$$subdir ; \
	  $(INSTALL) $$i $(DESTDIR)$(MANDIR)/$$subdir ; \
	done

clean:
	$(RM) $(MAN) $(MAN:%=%.html) *.[0-9].gendoc.txt MANIFEST

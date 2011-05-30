SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
GENDOC=gendoc.py ../mercurial/commands.py ../mercurial/help.py \
	../mercurial/help/*.txt ../hgext/*.py ../hgext/*/__init__.py
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
RSTARGS=

export LANGUAGE=C
export LC_ALL=C

all: man html

man: $(MAN)

html: $(HTML)

hg.1.txt: hg.1.gendoc.txt
	touch hg.1.txt

hg.1.gendoc.txt: $(GENDOC)
	${PYTHON} gendoc.py > $@.tmp
	mv $@.tmp $@

hgrc.5: ../mercurial/help/config.txt

hgrc.5.html: ../mercurial/help/config.txt

%: %.txt common.txt
	$(PYTHON) runrst hgmanpage $(RSTARGS) --halt warning \
	  --strip-elements-with-class htmlonly $*.txt $*

%.html: %.txt common.txt
	$(PYTHON) runrst html $(RSTARGS) --halt warning \
	  --link-stylesheet --stylesheet-path style.css $*.txt $*.html

MANIFEST: man html
# tracked files are already in the main MANIFEST
	$(RM) $@
	for i in $(MAN) $(HTML); do \
	  echo "doc/$$i" >> $@ ; \
	done

install: man
	for i in $(MAN) ; do \
	  subdir=`echo $$i | sed -n 's/^.*\.\([0-9]\)$$/man\1/p'` ; \
	  mkdir -p $(DESTDIR)$(MANDIR)/$$subdir ; \
	  $(INSTALL) $$i $(DESTDIR)$(MANDIR)/$$subdir ; \
	done

clean:
	$(RM) $(MAN) $(HTML) hg.1.gendoc.txt MANIFEST

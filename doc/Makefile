SOURCES=$(notdir $(wildcard ../mercurial/help/*.[0-9].txt))
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
GENDOC=gendoc.py ../mercurial/commands.py ../mercurial/help.py \
	../mercurial/help/*.txt ../hgext/*.py ../hgext/*/__init__.py
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
RSTARGS=

export HGENCODING=UTF-8

all: man html

man: $(MAN)

html: $(HTML)

common.txt $(SOURCES) $(SOURCES:%.txt=%.gendoc.txt): $(GENDOC)
	${PYTHON} gendoc.py $(basename $@) > $@.tmp
	mv $@.tmp $@

%: %.txt %.gendoc.txt common.txt
	$(PYTHON) runrst hgmanpage $(RSTARGS) --halt warning \
	  --strip-elements-with-class htmlonly $*.txt $*

%.html: %.txt %.gendoc.txt common.txt
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
	$(RM) $(MAN) $(HTML) common.txt $(SOURCES) $(SOURCES:%.txt=%.gendoc.txt) MANIFEST

SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
ASCIIDOC=asciidoc

all: man html

man: $(MAN)

html: $(HTML)

hg.1.txt: hg.1.gendoc.txt
	touch hg.1.txt

hg.1.gendoc.txt: ../mercurial/commands.py ../mercurial/help.py
	${PYTHON} gendoc.py > $@

%: %.xml
	xmlto man $*.xml && \
	sed -e 's/^\.hg/\\\&.hg/' $* > $*~ && \
	mv $*~ $*

%.xml: %.txt
	$(ASCIIDOC) -d manpage -b docbook $*.txt

%.html: %.txt
	$(ASCIIDOC) -b html4 $*.txt || $(ASCIIDOC) -b html $*.txt

MANIFEST: man html
	# tracked files are already in the main MANIFEST
	$(RM) $@
	for i in $(MAN) $(HTML) hg.1.gendoc.txt; do \
	  echo "doc/$$i" >> $@ ; \
	done

install: man
	for i in $(MAN) ; do \
	  subdir=`echo $$i | sed -n 's/..*\.\([0-9]\)$$/man\1/p'` ; \
	  mkdir -p $(DESTDIR)$(MANDIR)/$$subdir ; \
	  $(INSTALL) $$i $(DESTDIR)$(MANDIR)/$$subdir ; \
	done

clean:
	$(RM) $(MAN) $(MAN:%=%.xml) $(MAN:%=%.html) *.[0-9].gendoc.txt MANIFEST

SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
RST2HTML=rst2html
RST2MAN=rst2man

all: man html

man: $(MAN)

html: $(HTML)

hg.1.txt: hg.1.gendoc.txt
	touch hg.1.txt

hg.1.gendoc.txt: gendoc.py ../mercurial/commands.py ../mercurial/help.py
	${PYTHON} gendoc.py > $@

%: %.txt common.txt
	# add newline after all literal blocks and fix backslash escape
	$(RST2MAN) $*.txt \
	  | sed -e 's/^\.fi$$/.fi\n/' \
	  | sed -e 's/\\fB\\\\fP/\\fB\\e\\fP/' \
	  > $*

%.html: %.txt common.txt
	$(RST2HTML) $*.txt > $*.html

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

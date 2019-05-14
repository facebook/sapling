SOURCES=$(notdir $(wildcard ../edenscm/mercurial/help/*.[0-9].txt))
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)
PREFIX=/usr/local
MANDIR=$(PREFIX)/share/man
INSTALL=install -c -m 644
PYTHON=python
RSTARGS=

all: ;

install: ;

clean:
	$(RM) $(MAN) $(HTML) common.txt $(SOURCES) $(SOURCES:%.txt=%.gendoc.txt) MANIFEST

SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)

all: man

man: $(MAN)

html: $(HTML)

%: %.xml
	xmlto man $*.xml

%.xml: %.txt
	asciidoc -d manpage -b docbook $*.txt

%.html: %.txt
	asciidoc -b html $*.txt

clean:
	$(RM) $(MAN) $(MAN:%=%.xml) $(MAN:%=%.html)

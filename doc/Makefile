
SOURCES=$(wildcard *.1.txt)
MAN=$(SOURCES:%.1.txt=%.1)
HTML=$(SOURCES:%.1.txt=%.1.html)

all: man

man: $(MAN)

html: $(HTML)

%.1: %.1.xml
	xmlto man $*.1.xml

%.1.xml: %.1.txt
	asciidoc -d manpage -b docbook $*.1.txt

%.1.html: %.1.txt
	asciidoc -b html $*.1.txt

clean:
	$(RM) $(MAN) $(MAN:%.1=%.1.xml) $(MAN:%.1=%.1.html)

SOURCES=$(wildcard *.[0-9].ja.txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)

all: man html

man: $(MAN)

html: $(HTML)

%: %.xml
	xmlto -x docbook.ja.xsl man $*.xml

%.xml: %.txt
	-asciidoc -d manpage -b docbook -f docbook.ja.conf $*.txt

%.html: %.txt
	asciidoc -b html4 $*.txt

clean:
	$(RM) $(MAN:%.ja=%) $(MAN:%=%.xml) $(MAN:%=%.html)

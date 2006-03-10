SOURCES=$(wildcard *.[0-9].txt)
MAN=$(SOURCES:%.txt=%)
HTML=$(SOURCES:%.txt=%.html)

all: man html

man: $(MAN)

html: $(HTML)

hg.1.txt: hg.1.gendoc.txt
	touch hg.1.txt

hg.1.gendoc.txt: ../mercurial/commands.py
	python gendoc.py > $@

%: %.xml
	xmlto man $*.xml

%.xml: %.txt
	asciidoc -d manpage -b docbook $*.txt

%.html: %.txt
	asciidoc -b html4 $*.txt || asciidoc -b html $*.txt

clean:
	$(RM) $(MAN) $(MAN:%=%.xml) $(MAN:%=%.html) *.[0-9].gendoc.txt

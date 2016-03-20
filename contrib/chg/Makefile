HG = $(CURDIR)/../../hg

TARGET = chg
SRCS = chg.c hgclient.c util.c
OBJS = $(SRCS:.c=.o)

CFLAGS ?= -O2 -Wall -Wextra -pedantic -g
CPPFLAGS ?= -D_FORTIFY_SOURCE=2
override CFLAGS += -std=gnu99
ifdef HGPATH
override CPPFLAGS += -DHGPATH=\"$(HGPATH)\"
endif

DESTDIR =
PREFIX = /usr/local
MANDIR = $(PREFIX)/share/man/man1

CHGSOCKDIR = /tmp/chg$(shell id -u)
CHGSOCKNAME = $(CHGSOCKDIR)/server

.PHONY: all
all: $(TARGET)

$(TARGET): $(OBJS)
	$(CC) $(LDFLAGS) -o $@ $(OBJS)

chg.o: hgclient.h util.h
hgclient.o: hgclient.h util.h
util.o: util.h

.PHONY: install
install: $(TARGET)
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 $(TARGET) $(DESTDIR)$(PREFIX)/bin
	install -d $(DESTDIR)$(MANDIR)
	install -m 644 chg.1 $(DESTDIR)$(MANDIR)

.PHONY: serve
serve:
	[ -d $(CHGSOCKDIR) ] || ( umask 077; mkdir $(CHGSOCKDIR) )
	$(HG) serve --cwd / --cmdserver chgunix \
		--address $(CHGSOCKNAME) \
		--config extensions.chgserver= \
		--config cmdserver.log=/dev/stderr

.PHONY: clean
clean:
	$(RM) $(OBJS)

.PHONY: distclean
distclean:
	$(RM) $(OBJS) $(TARGET)

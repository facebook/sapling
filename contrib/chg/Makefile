TARGET = chg
SRCS = chg.c hgclient.c procutil.c util.c
OBJS = $(SRCS:.c=.o)

CFLAGS ?= -O2 -Wall -Wextra -pedantic -g
CPPFLAGS ?= -D_FORTIFY_SOURCE=2 -D_GNU_SOURCE
override CFLAGS += -std=gnu99
ifdef HGPATH
override CPPFLAGS += -DHGPATH=\"$(HGPATH)\"
endif

DESTDIR =
PREFIX = /usr/local
MANDIR = $(PREFIX)/share/man/man1

.PHONY: all
all: $(TARGET)

$(TARGET): $(OBJS)
	$(CC) $(LDFLAGS) -o $@ $(OBJS)

chg.o: hgclient.h procutil.h util.h
hgclient.o: hgclient.h procutil.h util.h
procutil.o: procutil.h util.h
util.o: util.h

.PHONY: install
install: $(TARGET)
	install -d "$(DESTDIR)$(PREFIX)"/bin
	install -m 755 "$(TARGET)" "$(DESTDIR)$(PREFIX)"/bin
	install -d "$(DESTDIR)$(MANDIR)"
	install -m 644 chg.1 "$(DESTDIR)$(MANDIR)"

.PHONY: clean
clean:
	$(RM) $(OBJS)

.PHONY: distclean
distclean:
	$(RM) $(OBJS) $(TARGET)

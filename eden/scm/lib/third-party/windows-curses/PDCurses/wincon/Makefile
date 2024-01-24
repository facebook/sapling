# GNU Makefile for PDCurses - Windows console
#
# Usage: make [-f path\Makefile] [DEBUG=Y] [DLL=Y] [WIDE=Y] [UTF8=Y]
#        [INFOEX=N] [tgt]
#
# where tgt can be any of:
# [all|demos|pdcurses.a|testcurs.exe...]

O = o
E = .exe

ifeq ($(OS),Windows_NT)
	RM = cmd /c del
else
	RM = rm -f
endif

ifndef PDCURSES_SRCDIR
	PDCURSES_SRCDIR = ..
endif

osdir		= $(PDCURSES_SRCDIR)/wincon
common		= $(PDCURSES_SRCDIR)/common

include $(common)/libobjs.mif

PDCURSES_WIN_H	= $(osdir)/pdcwin.h

CC		= gcc
AR		= ar
STRIP		= strip
LINK		= gcc
WINDRES		= windres

ifeq ($(DEBUG),Y)
	CFLAGS  = -g -Wall -DPDCDEBUG
	LDFLAGS = -g
else
	CFLAGS  = -O2 -Wall
	LDFLAGS =
endif

CFLAGS += -I$(PDCURSES_SRCDIR)

ifeq ($(WIDE),Y)
	CFLAGS += -DPDC_WIDE
endif

ifeq ($(UTF8),Y)
	CFLAGS += -DPDC_FORCE_UTF8
endif

ifeq ($(DLL),Y)
	CFLAGS += -DPDC_DLL_BUILD
	LIBEXE = $(CC)
	LIBFLAGS = -Wl,--out-implib,pdcurses.a -shared -o
	LIBCURSES = pdcurses.dll
	CLEAN = $(LIBCURSES) *.a
	RESOURCE = pdcurses.o
else
	LIBEXE = $(AR)
	LIBFLAGS = rcv
	LIBCURSES = pdcurses.a
	CLEAN = *.a
endif

ifeq ($(INFOEX),N)
	CFLAGS += -DHAVE_NO_INFOEX
endif

.PHONY: all libs clean demos dist

all:	libs

libs:	$(LIBCURSES)

clean:
	-$(RM) *.o
	-$(RM) *.exe
	-$(RM) $(CLEAN)

demos:	$(DEMOS)
ifneq ($(DEBUG),Y)
	$(STRIP) *.exe
endif

$(LIBCURSES) : $(LIBOBJS) $(PDCOBJS) $(RESOURCE)
	$(LIBEXE) $(LIBFLAGS) $@ $?

pdcurses.o: $(common)/pdcurses.rc
	$(WINDRES) -i $(common)/pdcurses.rc pdcurses.o

$(LIBOBJS) $(PDCOBJS) : $(PDCURSES_HEADERS)
$(PDCOBJS) : $(PDCURSES_WIN_H)
$(DEMOS) : $(PDCURSES_CURSES_H) $(LIBCURSES)
panel.o : $(PANEL_HEADER)

$(LIBOBJS) : %.o: $(srcdir)/%.c
	$(CC) -c $(CFLAGS) $<

$(PDCOBJS) : %.o: $(osdir)/%.c
	$(CC) -c $(CFLAGS) $<

firework.exe ozdemo.exe rain.exe testcurs.exe worm.exe xmas.exe \
ptest.exe: %.exe: $(demodir)/%.c
	$(CC) $(CFLAGS) -o$@ $< $(LIBCURSES)

tuidemo.exe: tuidemo.o tui.o
	$(LINK) $(LDFLAGS) -o$@ tuidemo.o tui.o $(LIBCURSES)

tui.o: $(demodir)/tui.c $(demodir)/tui.h $(PDCURSES_CURSES_H)
	$(CC) -c $(CFLAGS) -I$(demodir) -o$@ $<

tuidemo.o: $(demodir)/tuidemo.c $(PDCURSES_CURSES_H)
	$(CC) -c $(CFLAGS) -I$(demodir) -o$@ $<

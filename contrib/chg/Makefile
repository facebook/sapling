TARGET = chg
SRCS = chg.c hgclient.c procutil.c util.c
OBJS = $(SRCS:.c=.o)

CFLAGS ?= -O2 -Wall -Wextra -pedantic -g
CPPFLAGS ?= -D_FORTIFY_SOURCE=2 -D_GNU_SOURCE
override CFLAGS += -std=gnu99

.PHONY: all
all: static

.PHONY: static
static:
	$(CC) -c $(SRCS) $(CFLAGS) $(CPPFLAGS) -DCHG_STATIC_LIB -fPIC
	ar rcs lib$(TARGET).a $(OBJS)

chg.o: hgclient.h procutil.h util.h
hgclient.o: hgclient.h procutil.h util.h
procutil.o: procutil.h util.h
util.o: util.h

.PHONY: clean
clean:
	$(RM) $(OBJS)
	$(RM) $(TARGET).a

.PHONY: distclean
distclean:
	$(RM) $(OBJS) $(TARGET)

.PHONY: all clean

CFLAGS ?= -std=c99 -O2 -Wall -Wextra -Wconversion -pedantic -g

all: linelogcli

linelogcli: linelogcli.c linelog.c linelog.h
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $<

clean:
	rm -f linelogcli

.PHONY: all clean

CFLAGS ?= -std=c11 -O2 -Wall -Wextra -Wconversion -Wpedantic -g

all: linelogcli

linelogcli: linelogcli.c linelog.c linelog.h
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $<

clean:
	rm -f linelogcli

CC := gcc
CFLAGS := -g -O2 -Wall -Werror

prefix ?= /usr/bin

hgsh: hgsh.o
	$(CC) -o $@ $<

install: hgsh
	install -m755 hgsh $(prefix)

clean:
	rm -f *.o hgsh

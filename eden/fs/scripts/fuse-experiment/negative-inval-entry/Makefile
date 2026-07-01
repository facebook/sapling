CC ?= cc
CFLAGS := -Wall -O2 $(shell pkg-config --cflags fuse3)
LDFLAGS := $(shell pkg-config --libs fuse3) -lpthread

test: testfs
	python3 test_negative_inval_entry.py

testfs: testfs.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

clean:
	rm -f testfs

.PHONY: clean test

CC ?= cc
UNSHARE ?= unshare -Urm --map-root-user
CFLAGS := -Wall -O2 $(shell pkg-config --cflags fuse3)
LDFLAGS := $(shell pkg-config --libs fuse3) -lpthread

test: testfs
	$(UNSHARE) python3 test_bind_invalidate.py

testfs: testfs.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

clean:
	rm -f testfs

.PHONY: clean test

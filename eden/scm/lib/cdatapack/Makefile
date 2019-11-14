LDFLAGS = -llz4 -lcrypto
CC = gcc
CFLAGS = -g -std=c99 -Wall -Werror -I.. -I../..

all: cdatapack_dump cdatapack_get

cdatapack_dump: cdatapack_dump.o cdatapack.o

cdatapack_get: cdatapack_get.o cdatapack.o

%.o: %.c cdatapack.h
	$(CC) $(CFLAGS) -c -o $@ $<

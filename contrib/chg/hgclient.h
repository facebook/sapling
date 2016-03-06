/*
 * A command server client that uses Unix domain socket
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#ifndef HGCLIENT_H_
#define HGCLIENT_H_

#include <sys/types.h>

struct hgclient_tag_;
typedef struct hgclient_tag_ hgclient_t;

hgclient_t *hgc_open(const char *sockname);
void hgc_close(hgclient_t *hgc);

pid_t hgc_peerpid(const hgclient_t *hgc);

const char **hgc_validate(hgclient_t *hgc, const char *const args[],
			  size_t argsize);
int hgc_runcommand(hgclient_t *hgc, const char *const args[], size_t argsize);
void hgc_attachio(hgclient_t *hgc);
const char *hgc_getpager(hgclient_t *hgc, const char *const args[],
			 size_t argsize);
void hgc_setenv(hgclient_t *hgc, const char *const envp[]);

#endif  /* HGCLIENT_H_ */

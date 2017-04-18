/*
 * Utilities about process handling - signal and subprocess (ex. pager)
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#ifndef PROCUTIL_H_
#define PROCUTIL_H_

#include <unistd.h>

void restoresignalhandler(void);
void setupsignalhandler(pid_t pid, pid_t pgid);

pid_t setuppager(const char *pagercmd, const char *envp[]);
void waitpager(void);

#endif /* PROCUTIL_H_ */

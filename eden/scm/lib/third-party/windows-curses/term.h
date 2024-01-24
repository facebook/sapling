/* Public Domain Curses */

/* PDCurses doesn't operate with terminfo, but we need these functions for
   compatibility, to allow some things (notably, interface libraries for
   other languages) to be compiled. Anyone who tries to actually _use_
   them will be disappointed, since they only return ERR. */

#ifndef __PDCURSES_TERM_H__
#define __PDCURSES_TERM_H__ 1

#include <curses.h>

#if defined(__cplusplus) || defined(__cplusplus__) || defined(__CPLUSPLUS)
extern "C"
{
#endif

typedef struct
{
    const char *_termname;
} TERMINAL;

PDCEX  TERMINAL *cur_term;

PDCEX  int     del_curterm(TERMINAL *);
PDCEX  int     putp(const char *);
PDCEX  int     restartterm(const char *, int, int *);
PDCEX  TERMINAL *set_curterm(TERMINAL *);
PDCEX  int     setterm(const char *);
PDCEX  int     setupterm(const char *, int, int *);
PDCEX  int     tgetent(char *, const char *);
PDCEX  int     tgetflag(const char *);
PDCEX  int     tgetnum(const char *);
PDCEX  char   *tgetstr(const char *, char **);
PDCEX  char   *tgoto(const char *, int, int);
PDCEX  int     tigetflag(const char *);
PDCEX  int     tigetnum(const char *);
PDCEX  char   *tigetstr(const char *);
PDCEX  char   *tparm(const char *, long, long, long, long, long,
                     long, long, long, long);
PDCEX  int     tputs(const char *, int, int (*)(int));

#if defined(__cplusplus) || defined(__cplusplus__) || defined(__CPLUSPLUS)
}
#endif

#endif /* __PDCURSES_TERM_H__ */

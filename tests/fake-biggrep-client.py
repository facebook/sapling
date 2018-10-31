#!/usr/bin/env python
# This is a terribly anemic fake implementation of the biggrep client


# The null commit
NULL = "0" * 40

# Escape sequences used by biggrep_client
MAGENTA = "\x1b[35m\x1b[K"
OFF = "\x1b[m\x1b[K"
BLUE = "\x1b[36m\x1b[K"
GREEN = "\x1b[32m\x1b[K"


def magenta(what):
    return MAGENTA + what + OFF


def blue(what):
    return BLUE + what + OFF


def green(what):
    return GREEN + what + OFF


def result_line(filename, line, col, context):
    return (
        magenta(filename)
        + blue(":")
        + green(str(line))
        + blue(":")
        + green(str(col))
        + blue(":")
        + context
    )


print("#%s:0" % NULL)
print(result_line("notgrepdir/donotseeme", 10, 1, "lalala"))
print(result_line("grepdir/fakefile", 10, 1, "fakeresult"))

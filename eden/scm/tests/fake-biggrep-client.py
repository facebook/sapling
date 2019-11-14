#!/usr/bin/env python
# This is a terribly anemic fake implementation of the biggrep client
import argparse
import re
import subprocess


# The null commit
NULL = "0" * 40

# Escape sequences used by biggrep_client
MAGENTA = "\x1b[35m\x1b[K"
OFF = "\x1b[m\x1b[K"
BLUE = "\x1b[36m\x1b[K"
GREEN = "\x1b[32m\x1b[K"


parser = argparse.ArgumentParser()
parser.add_argument("--stripdir", action="store_true")
parser.add_argument("-r", action="store_true")
parser.add_argument("--color")
parser.add_argument("--expression")
parser.add_argument("-f")
parser.add_argument("tier")
parser.add_argument("corpus")
parser.add_argument("engine")
args = parser.parse_args()


def magenta(what):
    if args.color:
        return MAGENTA + what + OFF
    return what


def blue(what):
    if args.color:
        return BLUE + what + OFF
    return what


def green(what):
    if args.color:
        return GREEN + what + OFF
    return what


def result_line(filename, line, col, context):
    if args.f:
        if not re.match(args.f, filename):
            return

    if not re.match(args.expression, context):
        return

    print(
        magenta(filename)
        + blue(":")
        + green(str(line))
        + blue(":")
        + green(str(col))
        + blue(":")
        + context
        # stick _bg on the end so we can tell that the result
        # came from biggrep
        + "_bg"
    )


# Report the current commit as the corpus revision so that `hg grep` doesn't
# then need to go and run grep for itself over the files
p = subprocess.Popen(["hg", "log", "-r", ".", "-T{node}"], stdout=subprocess.PIPE)
out, err = p.communicate()
rev = out.rstrip()
print("#%s:0" % rev)

# This list is coupled with the "Set up the repository with some simple files"
# section of eden/scm/tests/test-fb-hgext-tweakdefaults-grep.t
files = {
    "grepdir/grepfile1": "foobarbaz",
    "grepdir/grepfile2": "foobarboo",
    "grepdir/subdir1/subfile1": "foobar_subdir",
    "grepdir/subdir2/subfile2": "foobar_dirsub",
}

for (filename, context) in files.items():
    result_line(filename, 1, 1, context)

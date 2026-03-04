#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This is a fake implementation of the biggrep client for testing.
#
# Environment variables:
#   BIGGREP_ARGS_FILE: If set, save the command-line arguments to this file
#   BIGGREP_CORPUS_REV: If set, use this revision as the corpus revision
#                       (otherwise uses current working directory parent)
#   BIGGREP_FILES: JSON object mapping filenames to their content (required)

import argparse
import json
import os
import re
import subprocess
import sys

# If BIGGREP_ARGS_FILE is set, save arguments to that file (but continue normal execution)
args_file = os.environ.get("BIGGREP_ARGS_FILE")
if args_file:
    with open(args_file, "w") as f:
        f.write(" ".join(sys.argv[1:]) + "\n")

# Escape sequences used by biggrep_client
MAGENTA = "\x1b[35m\x1b[K"
OFF = "\x1b[m\x1b[K"
BLUE = "\x1b[36m\x1b[K"
GREEN = "\x1b[32m\x1b[K"


parser = argparse.ArgumentParser()
parser.add_argument("--stripdir", action="store_true")
parser.add_argument("-r", action="store_true")
parser.add_argument("-l", action="store_true", help="Print only filenames with matches")
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

    if not re.search(args.expression.replace(r"\-", "-"), context):
        return

    if args.l:
        # In -l mode, print only the filename
        print(magenta(filename))
    else:
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


# If BIGGREP_CORPUS_REV is set, use that as the corpus revision.
# Otherwise, use the current commit so that `hg grep` doesn't
# need to run local grep.
corpus_rev = os.environ.get("BIGGREP_CORPUS_REV")
if corpus_rev:
    # Resolve the revision to a full node
    p = subprocess.Popen(
        ["hg", "log", "-r", corpus_rev, "-T{node}"], stdout=subprocess.PIPE
    )
    out, err = p.communicate()
    rev = out.rstrip().decode("utf-8")
else:
    p = subprocess.Popen(["hg", "log", "-r", ".", "-T{node}"], stdout=subprocess.PIPE)
    out, err = p.communicate()
    rev = out.rstrip().decode("utf-8")

print("#fake=%s:0" % rev)

# BIGGREP_FILES is required - JSON object mapping filenames to content
files_env = os.environ.get("BIGGREP_FILES")
if not files_env:
    print("error: BIGGREP_FILES environment variable is required", file=sys.stderr)
    sys.exit(1)

files = json.loads(files_env)

for filename, context in files.items():
    result_line(filename, 1, 1, context)

#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import shlex
import subprocess
import sys
import threading
from typing import Optional, TypeVar


_T = TypeVar("_T")


def none_throws(optional: Optional[_T]) -> _T:
    assert optional is not None, "Unexpected None"
    return optional


os.chdir(none_throws(os.getenv("TESTTMP")))


def parse(cmd):
    """
    matches hg-ssh-wrapper
    """
    try:
        return shlex.split(cmd)
    except ValueError as e:
        print('Illegal command "%s": %s\n' % (cmd, e), file=sys.stderr)
        sys.exit(255)


def parse_repo_path(path):
    """
    matches hg-ssh-wrapper
    """
    path = path.split("?")
    if len(path) == 1:
        repo = path[0]
        marker = None
    elif len(path) == 2:
        repo = path[0]
        marker = path[1]
    else:
        print("Illegal repo name: %s\n" % "?".join(path), file=sys.stderr)
        sys.exit(255)

    return repo, marker


# Skipping SSH options
host_index = 1
while host_index < len(sys.argv) and sys.argv[host_index].startswith("-"):
    host_index += 1

if sys.argv[host_index] != "user@dummy":
    sys.exit(-1)

os.environ["SSH_CLIENT"] = "%s 1 2" % os.environ.get(
    "SSH_IP_OVERRIDE", os.environ.get("LOCALIP", "127.0.0.1")
)

log = open("dummylog", "ab")
log.write(b"Got arguments")
for i, arg in enumerate(sys.argv[1:]):
    log.write(b" %d:%s" % (i + 1, arg.encode("utf-8")))
log.write(b"\n")
log.close()
hgcmd = sys.argv[host_index + 1]
if os.name == "nt":
    # hack to make simple unix single quote quoting work on windows
    hgcmd = hgcmd.replace("'", '"')

cmdargv = parse(hgcmd)
if cmdargv[:2] == ["hg", "-R"] and cmdargv[3:] == ["serve", "--stdio"]:
    path, marker = parse_repo_path(cmdargv[2])
    if marker == "read_copy":
        path = path + "_copy"
    cmdargv[2] = path
    hgcmd = subprocess.list2cmdline(cmdargv)

if os.environ.get("DUMMYSSH_STABLE_ORDER"):
    # Buffer all stderr outputs until the end of connection.  This reduces test
    # flakiness where stderr and stdout output order is nondeterministic.
    p = subprocess.Popen(hgcmd, shell=True, stderr=subprocess.PIPE)
    errbuf = [b""]

    def readstderr():
        while True:
            ch = p.stderr.read(1)
            if not ch:
                break
            errbuf[0] += ch

    t = threading.Thread(target=readstderr)
    t.start()

    p.wait()
    t.join()

    if sys.version_info[0] >= 3:
        sys.stderr.buffer.write(errbuf[0])
    else:
        sys.stderr.write(errbuf[0])
    sys.stderr.flush()

    sys.exit(p.returncode)
else:
    r = os.system(hgcmd)
    sys.exit(bool(r))

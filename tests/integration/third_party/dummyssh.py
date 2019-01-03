#!/usr/bin/env python

from __future__ import absolute_import

import os
import sys

os.chdir(os.getenv("TESTTMP"))

if sys.argv[1] != "user@dummy":
    sys.exit(-1)

os.environ["SSH_CLIENT"] = "%s 1 2" % os.environ.get("LOCALIP", "[::1]")

log = open("dummylog", "ab")
log.write("Got arguments")
for i, arg in enumerate(sys.argv[1:]):
    log.write(" %d:%s" % (i + 1, arg))
log.write("\n")
log.close()
hgcmd = sys.argv[2]
if os.name == "nt":
    # hack to make simple unix single quote quoting work on windows
    hgcmd = hgcmd.replace("'", '"')

log = open("dummylog", "a+b")

cert = os.path.join(os.getenv("TESTDIR"), "testcert.crt")
capem = os.path.join(os.getenv("TESTDIR"), "testcert.crt")
privatekey = os.path.join(os.getenv("TESTDIR"), "testcert.key")

if "hgcli" in hgcmd:
    hgcmd += (
        " --mononoke-path [::1]:"
        + os.getenv("MONONOKE_SOCKET")
        + (" --cert %s --ca-pem %s --private-key %s --common-name localhost" % (cert, capem, privatekey))
    )

r = os.system(hgcmd)
sys.exit(bool(r))

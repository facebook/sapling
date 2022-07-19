# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os.path
import sys
import time

from edenscm.mercurial import progress, registrar


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "basiccommandtest",
    [
        (
            "",
            "waitfile",
            "",
            "if set, wait for file before exitting",
        ),
    ],
    "hg basiccommandtest exit_code",
    norepo=True,
)
def basiccommandtest(ui, exit_code, **opts):
    waitforfile(opts.get("waitfile"))
    sys.exit(int(exit_code))


@command(
    "progresstest",
    [
        (
            "",
            "waitfile",
            "",
            "if set, wait for file to exist before updating progress",
        ),
    ],
    "hg progresstest total",
    norepo=True,
)
def progresstest(ui, total, **opts):
    total = int(total)

    waitforfile(opts.get("waitfile"))

    with progress.bar(ui, "eating", "apples", total) as bar:
        for i in range(1, total + 1):
            bar.value = i
            waitforfile(opts.get("waitfile"))


def waitforfile(path):
    if not path:
        return

    while not os.path.exists(path):
        time.sleep(0.001)

    os.unlink(path)

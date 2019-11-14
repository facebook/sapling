# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401
from edenscm.mercurial.extensions import wrappedfunction

import time


# Setup repo
sh.newrepo()

now = int(time.time())

sh % "touch file1"
sh % "hg add file1"

for delta in [31536000, 86401, 86369, 3800, 420, 5]:
    committime = now - delta
    open("file1", "w").write("%s\n" % delta)
    sh.hg("commit", "-d", "%s 0" % committime, "-m", "Changeset %s seconds ago" % delta)

with wrappedfunction(time, "time", lambda orig: now + 1):
    # Check age ranges
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<30\")'" == "5 Changeset 5 seconds ago"
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<7m30s\")'" == r"""
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<1h4m\")'" == r"""
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<1d\")'" == r"""
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<364d23h59m\")'" == r"""
        1 Changeset 86401 seconds ago
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">1s\")'" == r"""
        0 Changeset 31536000 seconds ago
        1 Changeset 86401 seconds ago
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">1m\")'" == r"""
        0 Changeset 31536000 seconds ago
        1 Changeset 86401 seconds ago
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">1h\")'" == r"""
        0 Changeset 31536000 seconds ago
        1 Changeset 86401 seconds ago
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">1d\")'" == r"""
        0 Changeset 31536000 seconds ago
        1 Changeset 86401 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">365d\")'" == "0 Changeset 31536000 seconds ago"
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<64m\")'" == r"""
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<60m500s\")'" == r"""
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<1h500s\")'" == r"""
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"1h-20d\")'" == r"""
        1 Changeset 86401 seconds ago
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"invalid\")'" == r"""
        hg: parse error: invalid age range
        [255]"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"1h\")'" == r"""
        hg: parse error: invalid age range
        [255]"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"<3m2h\")'" == r"""
        hg: parse error: invalid age in age range: 3m2h
        [255]"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\">3h2h\")'" == r"""
        hg: parse error: invalid age in age range: 3h2h
        [255]"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'age(\"1h-5h-10d\")'" == r"""
        hg: parse error: invalid age in age range: 5h-10d
        [255]"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'ancestorsaged(., \"<1d\")'" == r"""
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago
        5 Changeset 5 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'ancestorsaged(.^, \"<1d\")'" == r"""
        2 Changeset 86369 seconds ago
        3 Changeset 3800 seconds ago
        4 Changeset 420 seconds ago"""
    sh % "hg log -T '{rev} {desc}\\n' -r 'ancestorsaged(., \"1d-20d\")'" == "1 Changeset 86401 seconds ago"
    sh % "hg log -T '{rev} {desc}\\n' -r 'ancestorsaged(., \">1d\")'" == r"""
        0 Changeset 31536000 seconds ago
        1 Changeset 86401 seconds ago"""

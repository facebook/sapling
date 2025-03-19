# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import re

from ..t.runtime import TestTmp


def testsetup(t: TestTmp):
    edenfsctl_path = t.requireexe("eden")
    # Make other parts of the tests aware of the fact the test is using EdenFS
    t.setenv("HGTEST_USE_EDEN", "1")
    # This fallback is there increasing the compatibility of the `goto` command.
    # EdenFS does not report the number of files that were updated, while the
    # rest of DotSL modes do.
    t.registerfallbackmatch(
        lambda a, b: (
            b == "update complete"
            or re.match(
                r"[0-9]+ files merged, [0-9]+ files unresolved",
                b,
            )
        )
        and re.match(
            r"[0-9]+ files updated, [0-9]+ files merged, [0-9]+ files removed, [0-9]+ files unresolved",
            a,
        )
    )
    # Required for using `--eden` in the `clone` command
    with open(t.getenv("SL_CONFIG_PATH"), "a", encoding="utf-8") as sl_config:
        sl_config.write(f"""
[edenfs]
command={edenfsctl_path}
backing-repos-dir=$TESTTMP/.eden-backing-repos

[clone]
use-eden=True
""")
    # Like $TESTTMP, but for the $EDENFS directory. See comments on
    # `tests/edenfs.py` to see why $EDENFS is not inside $TESTTMP
    edenfstmp = os.getenv("EDENFSTMP")
    t.setenv("EDENFSTMP", edenfstmp)
    t.substitutions.append((re.escape(edenfstmp), "$EDENFSTMP"))

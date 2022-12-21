# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import doctest
import os
import re
import sys

from hghave import require


# this is hack to make sure no escape characters are inserted into the output

if "TERM" in os.environ:
    del os.environ["TERM"]


def testmod(name, optionflags=0, testtarget=None):
    __import__(name)
    mod = sys.modules[name]
    if testtarget is not None:
        mod = getattr(mod, testtarget)

    # minimal copy of doctest.testmod()
    finder = doctest.DocTestFinder()
    checker = None
    runner = doctest.DocTestRunner(checker=checker, optionflags=optionflags)
    for test in finder.find(mod, name):
        runner.run(test)
    runner.summarize()


testmod("edenscm.ext.github.archive_commit")
testmod("edenscm.ext.github.github_repo_util")
testmod("edenscm.ext.github.pr_parser")
testmod("edenscm.ext.github.pull_request_arg")
testmod("edenscm.ext.github.pull_request_body")
testmod("edenscm.ext.github.templates")
testmod("edenscm.changegroup")
testmod("edenscm.changelog")
testmod("edenscm.cloneuri")
testmod("edenscm.cmdutil")
testmod("edenscm.color")
testmod("edenscm.config")
testmod("edenscm.context")
testmod("edenscm.dagparser", optionflags=doctest.NORMALIZE_WHITESPACE)
testmod("edenscm.dispatch")
testmod("edenscm.drawdag")
testmod("edenscm.encoding")
testmod("edenscm.formatter")
testmod("edenscm.gituser")
testmod("edenscm.hg")
testmod("edenscm.match")
testmod("edenscm.mdiff")
testmod("edenscm.minirst")
testmod("edenscm.mutation")
testmod("edenscm.patch")
testmod("edenscm.pathutil")
testmod("edenscm.parser")
testmod("edenscm.pycompat")
testmod("edenscm.result")
testmod("edenscm.revset")
testmod("edenscm.revsetlang")
testmod("edenscm.scmutil")
testmod("edenscm.smartset")
testmod("edenscm.store")
testmod("edenscm.templatefilters")
testmod("edenscm.templater")
testmod("edenscm.ui")
testmod("edenscm.uiconfig")
testmod("edenscm.url")
testmod("edenscm.util")
testmod("edenscm.util", testtarget="platform")
testmod("edenscm.ext.commitcloud.sync")

if sys.platform in {"linux", "win32"}:
    testmod("edenscm.testing.sh")
    testmod("edenscm.testing.t.diff")
    testmod("edenscm.testing.t.runtime")
    testmod("edenscm.testing.t.transform")

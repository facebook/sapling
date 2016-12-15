# __init__.py - asv benchmark suite
#
# Copyright 2016 Logilab SA <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# "historical portability" policy of contrib/benchmarks:
#
# We have to make this code work correctly with current mercurial stable branch
# and if possible with reasonable cost with early Mercurial versions.

'''ASV (https://asv.readthedocs.io) benchmark suite

Benchmark are parameterized against reference repositories found in the
directory pointed by the REPOS_DIR environment variable.

Invocation example:

    $ export REPOS_DIR=~/hgperf/repos
    # run suite on given revision
    $ asv --config contrib/asv.conf.json run REV
    # run suite on new changesets found in stable and default branch
    $ asv --config contrib/asv.conf.json run NEW
    # display a comparative result table of benchmark results between two given
    # revisions
    $ asv --config contrib/asv.conf.json compare REV1 REV2
    # compute regression detection and generate ASV static website
    $ asv --config contrib/asv.conf.json publish
    # serve the static website
    $ asv --config contrib/asv.conf.json preview
'''

from __future__ import absolute_import

import functools
import os
import re

from mercurial import (
    extensions,
    hg,
    ui as uimod,
    util,
)

basedir = os.path.abspath(os.path.join(os.path.dirname(__file__),
                          os.path.pardir, os.path.pardir))
reposdir = os.environ['REPOS_DIR']
reposnames = [name for name in os.listdir(reposdir)
              if os.path.isdir(os.path.join(reposdir, name, ".hg"))]
if not reposnames:
    raise ValueError("No repositories found in $REPO_DIR")
outputre = re.compile((r'! wall (\d+.\d+) comb \d+.\d+ user \d+.\d+ sys '
                       r'\d+.\d+ \(best of \d+\)'))

def runperfcommand(reponame, command, *args, **kwargs):
    os.environ["HGRCPATH"] = os.environ.get("ASVHGRCPATH", "")
    # for "historical portability"
    # ui.load() has been available since d83ca85
    if util.safehasattr(uimod.ui, "load"):
        ui = uimod.ui.load()
    else:
        ui = uimod.ui()
    repo = hg.repository(ui, os.path.join(reposdir, reponame))
    perfext = extensions.load(ui, 'perfext',
                              os.path.join(basedir, 'contrib', 'perf.py'))
    cmd = getattr(perfext, command)
    ui.pushbuffer()
    cmd(ui, repo, *args, **kwargs)
    output = ui.popbuffer()
    match = outputre.search(output)
    if not match:
        raise ValueError("Invalid output {0}".format(output))
    return float(match.group(1))

def perfbench(repos=reposnames, name=None, params=None):
    """decorator to declare ASV benchmark based on contrib/perf.py extension

    An ASV benchmark is a python function with the given attributes:

    __name__: should start with track_, time_ or mem_ to be collected by ASV
    params and param_name: parameter matrix to display multiple graphs on the
    same page.
    pretty_name: If defined it's displayed in web-ui instead of __name__
    (useful for revsets)
    the module name is prepended to the benchmark name and displayed as
    "category" in webui.

    Benchmarks are automatically parameterized with repositories found in the
    REPOS_DIR environment variable.

    `params` is the param matrix in the form of a list of tuple
    (param_name, [value0, value1])

    For example [(x, [a, b]), (y, [c, d])] declare benchmarks for
    (a, c), (a, d), (b, c) and (b, d).
    """
    params = list(params or [])
    params.insert(0, ("repo", repos))

    def decorator(func):
        @functools.wraps(func)
        def wrapped(repo, *args):
            def perf(command, *a, **kw):
                return runperfcommand(repo, command, *a, **kw)
            return func(perf, *args)

        wrapped.params = [p[1] for p in params]
        wrapped.param_names = [p[0] for p in params]
        wrapped.pretty_name = name
        return wrapped
    return decorator

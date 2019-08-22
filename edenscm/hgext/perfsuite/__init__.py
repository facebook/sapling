# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import random
import subprocess
import time

from edenscm.mercurial import (
    commands,
    encoding,
    error,
    metrics,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from . import editsgenerator


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-hgext"


class perftestsuite(object):
    """
    A simple integration test suite that runs against an existing repo, with the
    goal of logging perf numbers to CI.
    """

    def __init__(self, repo, publish=False, profile=False, printout=False):
        self.repo = repo
        self.ui = repo.ui
        self.publish = publish
        self.profile = profile
        self.printout = printout
        self.order = [
            "commit",
            "amend",
            "status",
            "revert",
            "rebase",
            "immrebase",
            "pull",
        ]
        self.editsgenerator = editsgenerator.randomeditsgenerator(repo[None])

        if profile:
            self.profileargs = ["--profile"]
        else:
            self.profileargs = []

    def run(self):
        for cmd in self.order:
            func = getattr(self, "test" + cmd)
            func()

    def _runtimeprofile(self, cmd, args=None, metricname=None):
        """Runs ``cmd`` with ``args`` and sends th result to ODS; also adds
        --profile, if requested."""
        if args is None:
            args = []
        if metricname is None:
            metricname = cmd
        with self.time(metricname):
            self._run([cmd] + args + self.profileargs)

    @contextlib.contextmanager
    def time(self, cmd):
        """Times the given block and logs that time to ODS"""
        reponame = self.ui.config("remotefilelog", "reponame")
        if not reponame:
            raise error.Abort(_("must set remotefilelog.reponame"))
        start = time.time()
        try:
            yield
        finally:
            duration = time.time() - start
            self.ui.warn(_("ran '%s' in %0.2f sec\n") % (cmd, duration))
            if self.publish:
                metrics.client(self.ui).gauge(
                    "%s.time" % cmd, duration, entity="hg.perfsuite.%s" % reponame
                )

    def testcommit(self):
        self.editsgenerator.makerandomedits(self.repo[None])
        self._run(["status"])
        self._run(["addremove"])
        self._runtimeprofile("commit", ["-m", "test commit"])

    def testamend(self):
        self.editsgenerator.makerandomedits(self.repo[None])
        self._run(["status"])
        self._run(["addremove"])
        self._runtimeprofile("amend")

    def teststatus(self):
        self.editsgenerator.makerandomedits(self.repo[None])
        self._runtimeprofile("status")

    def testrevert(self):
        self.editsgenerator.makerandomedits(self.repo[None])
        self._runtimeprofile("revert", ["--all"])

    def testpull(self):
        # TODO: Log the master rev at start, (real)strip N commits, then pull
        # that rev, to reduce the variability.
        self._runtimeprofile("pull")

    def testrebase(self):
        dist = self.ui.configint("perfsuite", "rebase.masterdistance", 1000)
        self._runtimeprofile("rebase", ["-s", ". % master", "-d", "master~%d" % dist])

    def testimmrebase(self):
        dist = self.ui.configint("perfsuite", "immrebase.masterdistance", 100)
        self._run(["update", "-C", "master"])
        configs = {("rebase", "experimental.inmemory"): True}
        with self.ui.configoverride(configs):
            self._runtimeprofile(
                "rebase",
                ["-r", "draft()", "-d", "master~%d" % dist],
                metricname="immrebase",
            )

    def _run(
        self,
        cmd,
        cwd=None,
        env=None,
        stderr=False,
        utf8decode=True,
        input=None,
        timeout=0,
        returncode=False,
    ):
        """Adapted from fbcode/scm/lib/_repo.py:Repository::run"""
        cmd = [util.hgexecutable(), "-R", self.repo.origroot] + cmd
        stdin = None
        if input:
            stdin = subprocess.PIPE
        p = self._spawn(
            cmd,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            stdin=stdin,
            timeout=timeout,
        )
        if input:
            if not isinstance(input, bytes):
                input = input.encode("utf-8")
            out, err = p.communicate(input=input)
        else:
            out, err = p.communicate()

        if out is not None and utf8decode:
            out = out.decode("utf-8")
        if err is not None and utf8decode:
            err = err.decode("utf-8")

        if p.returncode != 0 and returncode is False:
            self.ui.warn(_("run call failed!\n"))
            # Sometimes git or hg error output can be very big.
            # Let's limit stderr and stdout to 1000
            OUTPUT_LIMIT = 1000
            out = out[:OUTPUT_LIMIT]
            err = err[:OUTPUT_LIMIT]
            out = "STDOUT: %s\nSTDERR: %s\n" % (out, err)
            cmdstr = " ".join([self._safe_bytes_to_str(entry) for entry in cmd])
            cmdstr += "\n%s" % out
            ex = subprocess.CalledProcessError(p.returncode, cmdstr)
            ex.output = out
            raise ex

        if out and self.printout:
            self.ui.warn(_("stdout: %s\n") % out)
        if err and self.printout:
            self.ui.warn(_("stderr: %s\n") % err)

        if returncode:
            return out, err, p.returncode

        if stderr:
            return out, err, None
        return out, "", None

    def _safe_bytes_to_str(self, val):
        if isinstance(val, bytes):
            val = val.decode("utf-8", "replace")
        return val

    def _spawn(
        self,
        cmd,
        cwd=None,
        env=None,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        stdin=None,
        timeout=0,
    ):
        """Adapted from fbcode/scm/lib/_repo.py:Repository::spawn"""
        environ = encoding.environ.copy()
        if env:
            environ.update(env)

        if timeout != 0:
            timeoutcmd = ["timeout", str(timeout)]
            timeoutcmd.extend(cmd)
            cmd = timeoutcmd

        return subprocess.Popen(
            cmd,
            stdin=stdin,
            stdout=stdout,
            stderr=stderr,
            cwd=cwd,
            env=environ,
            close_fds=True,
        )


@command(
    "perftestsuite",
    [
        ("r", "rev", "", _("rev to update to first")),
        ("", "publish", False, _("whether to publish the metrics")),
        ("", "use-profile", False, _("whether to run commands in profile mode")),
        ("", "print", False, _("whether to print commands' stdout and stderr")),
        ("", "seed", 0, _("random seed to use")),
    ],
    _("hg perftestsuite"),
)
def perftestsuitecmd(ui, repo, *revs, **opts):
    """Runs an in-depth performance suite and logs results to a metrics
    framework.

    The rebase distance is configurable::

        [perfsuite]
        rebase.masterdistance = 100
        immrebase.masterdistance = 100

    The metrics endpoint is configurable::

        [ods]
        endpoint = https://somehost/metrics
    """
    if opts["seed"]:
        random.seed(opts["seed"])

    if opts["rev"]:
        ui.status(_("updating to %s...\n") % (opts["rev"]))
        commands.update(ui, repo, scmutil.revsingle(repo, opts["rev"]).hex())

    suite = perftestsuite(
        repo,
        publish=opts["publish"],
        profile=opts["use_profile"],
        printout=opts["print"],
    )
    suite.run()

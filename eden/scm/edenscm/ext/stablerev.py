# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""provide a way to expose the "stable" commit via a revset

In some repos, newly pushed commits undergo CI testing continuously. This means
`master` is often in an unknown state until it's tested; an older public commit
(e.g. `master~N`) will be the newest known-good commit. This extension will call
this the "stable" commit.

Using this stable commit instead of master can be useful during development
(e.g. when rebasing, to prevent rebasing onto a broken commit). This extension
provides a revset to access it easily.

The actual implementation of fetching the stable commit hash is left up to the
repository owner. Since the returned hash may not be in the repo, the revset
can optionally pull if the returned commit isn't known locally.

Lastly, it supports taking an optional argument (the "target") that's passed to
the script. This is useful for large repos that contain multiple projects, and
thus multiple stable commits.
"""

import re
import subprocess

from edenscm import commands, encoding, error, json, pycompat, registrar, util
from edenscm.i18n import _
from edenscm.revsetlang import getargsdict, getstring
from edenscm.smartset import baseset
from edenscm.templatekw import hybridlist

revsetpredicate = registrar.revsetpredicate()
namespacepredicate = registrar.namespacepredicate()
templatekeyword = registrar.templatekeyword()

# revspecs can be hashes, rev numbers, bookmark/tag names, etc., so this should
# be permissive:
validrevspec = re.compile(r"([0-9a-z\-_]+)", re.IGNORECASE)


def _getargumentornone(x):
    try:
        return getstring(x, _("must pass a target"))
    except error.ParseError:
        return None


def _validatetarget(ui, target):
    # The "target" parameter can be used or not depending on the configuration.
    # Can be "required", "optional", or "forbidden"
    targetconfig = ui.config("stablerev", "targetarg", "forbidden")
    if target is None and targetconfig == "required":
        raise error.Abort(_("must pass a target"))
    elif target is not None and targetconfig == "forbidden":
        raise error.Abort(_("targets are not supported in this repo"))

    return target


def _validaterevspec(ui, node):
    """Verifies the given node looks like a revspec"""
    script = ui.config("stablerev", "script")

    if len(node) == 0:
        raise error.Abort(_("stable rev returned by script (%s) was empty") % script)

    if not validrevspec.match(node):
        raise error.Abort(_("stable rev returned by script (%s) was invalid") % script)

    return node


def _executeandparse(ui, repo, target=None):
    script = ui.config("stablerev", "script")
    if script is None:
        raise error.ConfigError(_("must set stablerev.script"))

    # Pass '--target $TARGET' for compatibility.
    # XXX: Remove this once the new code has been rolled out for some time.
    env = {}
    if target is not None:
        script += " --target %s" % util.shellquote(target)
        env["TARGET"] = target

    stdout = _executescript(script, repo, env)

    try:
        # Prefer JSON output first.
        data = json.loads(stdout)
        if "node" in data:
            return _validaterevspec(ui, data["node"])
    except Exception:
        pass

    # Fall back to stdout:
    return _validaterevspec(ui, stdout.strip())


def _lookup(ui, repo, revspec, trypull=False):
    try:
        return repo[revspec]
    except error.RepoLookupError:
        if trypull:
            ui.warn(
                _("stable commit (%s) not in repo; pulling to get it...\n") % revspec
            )
            commands.pull(repo.ui, repo)

            # Rerun with trypull=False so we'll give up if it doesn't exist.
            return _lookup(ui, repo, revspec, trypull=False)
        else:
            raise error.Abort(
                _("stable commit (%s) not in the repo") % revspec,
                hint=_("try @prog@ pull first"),
            )


@revsetpredicate("getstablerev([target])", safe=False, weight=30)
def getstablerev(repo, subset, x):
    """Returns the "stable" revision.

    .. container:: verbose

        The script to run is set via config::

            [stablerev]
            script = scripts/get_stable_rev.py

        The revset takes an optional "target" argument that is passed to the
        script (as `--target $TARGET`). This argumement can be made `optional`,
        `required`, or `forbidden`::

            [stablerev]
            targetarg = forbidden

        The revset can automatically pull if the returned commit doesn't exist
        locally::

            [stablerev]
            pullonmissing = False
    """
    ui = repo.ui
    target = None
    args = getargsdict(x, "getstablerev", "target")
    if "target" in args:
        target = getstring(args["target"], _("target argument must be a string"))

    _validatetarget(ui, target)
    revspec = _executeandparse(ui, repo, target)
    trypull = ui.configbool("stablerev", "pullonmissing", False)
    commitctx = _lookup(ui, repo, revspec, trypull=trypull)

    return subset & baseset([commitctx.rev()], repo=repo)


@templatekeyword("stables")
def stables(repo, ctx, *args, **kwargs):
    """List of strings. Any stables associated with this revision.

    .. container:: verbose

        To get the list of stables, this calls stablerev.stablesscript with
        the commit nodeid. The called command is expected to output json with
        the following structure::

            {
                "<nodeid>": ["<stables>", ...]
            }

        stablerev.stablesscript should be a shell command with {nodeid} located in the
        command where the nodeid should be substituted::

            [stablerev]
            stablesscript = identify_stables {nodeid}
    """
    return hybridlist(_lookupstables(repo, ctx), name="stable")


def _lookupstables(repo, ctx):
    ui = repo.ui

    stablesscript = ui.config("stablerev", "stablesscript")
    if stablesscript is None:
        raise error.ConfigError(_("must set stablerev.stablesscript"))

    stablesscript = stablesscript.format(nodeid=util.shellquote(ctx.hex()))

    stdout = _executescript(stablesscript, repo)

    try:
        committostables = json.loads(stdout)
    except Exception as e:
        raise error.Abort(_("couldn't parse stablesscript stdout as json: %s") % e)

    return committostables.get(ctx.hex(), [])


def _executescript(script, repo, extraenv=None):
    ui = repo.ui

    reporoot = repo.wvfs.join("")
    script = repo.wvfs.join(script)

    ui.debug("Executing script: %s\n" % script)

    env = encoding.environ.copy()
    env.update({"REAL_CWD": pycompat.getcwd(), "HG_ROOT": reporoot})
    if extraenv:
        env.update(extraenv)
    ui.debug("setting current working directory to: %s\n" % reporoot)

    try:
        p = subprocess.Popen(
            args=script,
            shell=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=util.closefds,
            cwd=reporoot,
            env=env,
        )
        res = p.communicate()
    except subprocess.CalledProcessError as e:
        raise error.Abort(_("error executing script: %s") % e)

    stdout = pycompat.decodeutf8(res[0])
    stderr = pycompat.decodeutf8(res[1])

    ui.debug("script stdout:\n%s\n" % stdout)
    ui.write_err(stderr)

    if p.returncode:
        error.Abort(_("script returned non-zero return code: %d") % p.returncode)

    return stdout

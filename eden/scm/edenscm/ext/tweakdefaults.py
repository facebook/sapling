# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""user friendly defaults

This extension changes defaults to be more user friendly.

  @prog@ bookmark   always use unfiltered repo (--hidden)
  @prog@ log        always follows history (-f)
  @prog@ rebase     aborts without arguments
  @prog@ goto     aborts without arguments
  @prog@ grep       greps the working directory instead of history
  @prog@ histgrep   renamed from grep

Config::

    [tweakdefaults]
    # default destination used by pull --rebase / --update
    defaultdest = ''

    # whether to keep the commit date when doing amend / graft / rebase /
    # histedit
    amendkeepdate = False
    graftkeepdate = False
    rebasekeepdate = False
    histeditkeepdate = False

    # whether to show a warning or abort on some deprecated usages
    singlecolonwarn = False
    singlecolonabort = False

    # educational messages
    bmnodesthint = ''
    bmnodestmsg = ''
    nodesthint = ''
    nodestmsg = ''
    singlecolonmsg = ''

    # output new hashes when nodes get updated
    showupdated = False

    [grep]
    # Use external grep index
    usebiggrep = False
"""
from __future__ import absolute_import

import json
import re
import subprocess
import sys
import time
from typing import List, Set, Tuple

from edenscm import (
    bookmarks,
    commands,
    encoding,
    error,
    extensions,
    hg,
    patch,
    pycompat,
    registrar,
    revsetlang,
    scmutil,
    templatekw,
    templater,
    util,
)
from edenscm.i18n import _
from edenscm.node import short

from . import rebase


wrapcommand = extensions.wrapcommand
wrapfunction = extensions.wrapfunction

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-ext"

globaldata = "globaldata"
createmarkersoperation = "createmarkersoperation"

logopts: List[Tuple[str, str, None, str]] = [
    ("", "all", None, _("shows all changesets in the repo"))
]

configtable = {}
configitem = registrar.configitem(configtable)

configitem("grep", "command", default="xargs -0 grep")
configitem(globaldata, createmarkersoperation, default=None)

configitem("tweakdefaults", "singlecolonabort", default=False)
configitem("tweakdefaults", "singlecolonwarn", default=False)
configitem("tweakdefaults", "showupdated", default=False)

configitem("tweakdefaults", "amendkeepdate", default=False)
configitem("tweakdefaults", "graftkeepdate", default=False)
configitem("tweakdefaults", "histeditkeepdate", default=False)
configitem("tweakdefaults", "rebasekeepdate", default=False)
configitem("tweakdefaults", "absorbkeepdate", default=False)

rebasemsg: str = _(
    "you must use a bookmark with tracking "
    "or manually specify a destination for the rebase"
)
configitem(
    "tweakdefaults",
    "bmnodesthint",
    default=_(
        "set up tracking with `@prog@ book -t <destination>` "
        "or manually supply --dest / -d"
    ),
)
configitem("tweakdefaults", "bmnodestmsg", default=rebasemsg)
configitem(
    "tweakdefaults",
    "nodesthint",
    default=_(
        "set up tracking with `@prog@ book <name> -t <destination>` "
        "or manually supply --dest / -d"
    ),
)
configitem("tweakdefaults", "nodestmsg", default=rebasemsg)
configitem("tweakdefaults", "singlecolonmsg", default=_("use of ':' is deprecated"))


def uisetup(ui) -> None:
    tweakorder()


def extsetup(ui) -> None:
    wrapblame()

    entry = wrapcommand(commands.table, "commit", commitcmd)
    wrapcommand(rebase.cmdtable, "rebase", _rebase)
    wrapfunction(scmutil, "cleanupnodes", cleanupnodeswrapper)
    entry = wrapcommand(commands.table, "pull", pull)
    options = entry[1]
    options.append(("d", "dest", "", _("destination for rebase or update")))

    # anonymous function to pass ui object to _analyzewrapper
    def _analyzewrap(orig, x):
        return _analyzewrapper(orig, x, ui)

    wrapfunction(revsetlang, "_analyze", _analyzewrap)

    try:
        rebaseext = extensions.find("rebase")
        # tweakdefaults is already loaded before other extensions
        # (see tweakorder() function) so if these functions are wrapped
        # by something else, it's not a problem.
        wrapfunction(
            rebaseext, "_computeobsoletenotrebased", _computeobsoletenotrebasedwrapper
        )
        wrapfunction(rebaseext, "_checkobsrebase", _checkobsrebasewrapper)
    except KeyError:
        pass  # no rebase, no problem
    except AssertionError:
        msg = _(
            "tweakdefaults: _computeobsoletenotrebased or "
            + "_checkobsrebase are not what we expect them to be"
        )
        ui.warning(msg)

    try:
        remotenames = extensions.find("remotenames")
        wrapfunction(remotenames, "_getrebasedest", _getrebasedest)
    except KeyError:
        pass  # no remotenames, no worries
    except AttributeError:
        pass  # old version of remotenames doh

    entry = wrapcommand(commands.table, "log", log)
    for opt in logopts:
        opt = (opt[0], opt[1], opt[2], opt[3])
        entry[1].append(opt)

    entry = wrapcommand(commands.table, "status", statuscmd)
    options = entry[1]
    options.append(("", "root-relative", None, _("show status relative to root")))

    wrapcommand(commands.table, "graft", graftcmd)
    try:
        amendmodule = extensions.find("amend")
        wrapcommand(amendmodule.cmdtable, "amend", amendcmd)
    except KeyError:
        pass

    try:
        amendmodule = extensions.find("absorb")
        wrapcommand(amendmodule.cmdtable, "absorb", absorbcmd)
    except KeyError:
        pass

    try:
        histeditmodule = extensions.find("histedit")
        wrapfunction(histeditmodule, "commitfuncfor", histeditcommitfuncfor)
    except KeyError:
        pass

    # bookmark -D is an alias to strip -B

    # wrap bookmarks after remotenames
    def afterloaded(loaded):
        if loaded:
            # remotenames is loaded, wrap its wrapper directly
            remotenames = extensions.find("remotenames")
            wrapfunction(remotenames, "exbookmarks", unfilteredcmd)
            wrapfunction(remotenames, "expullcmd", pullrebaseffwd)
        else:
            # otherwise wrap the bookmark command
            wrapcommand(commands.table, "bookmark", unfilteredcmd)

    extensions.afterloaded("remotenames", afterloaded)

    entry = wrapcommand(commands.table, "diff", diffcmd)
    options = entry[1]
    options.append(
        (
            "",
            "per-file-stat-json",
            None,
            _("show diff stat per file in json (ADVANCED)"),
        )
    )

    pipei_bufsize = ui.configint("experimental", "winpipebufsize", 4096)
    if pipei_bufsize != 4096 and pycompat.iswindows:
        wrapfunction(util, "popen4", get_winpopen4(pipei_bufsize))

    _fixpager(ui)

    # Change manifest template output
    templatekw.defaulttempl["manifest"] = "{node}"


def reposetup(ui, repo) -> None:
    _fixpager(ui)


def tweakorder() -> None:
    """
    Tweakdefaults generally should load first; other extensions may modify
    behavior such that tweakdefaults will be happy, so we should not prevent
    that from happening too early in the process. Note that by loading first,
    we ensure that tweakdefaults's function wrappers run *last*.

    As of this writing, the extensions that we should load before are
    remotenames and directaccess (NB: directaccess messes with order as well).
    """
    order = extensions._order
    order.remove("tweakdefaults")
    order.insert(0, "tweakdefaults")
    extensions._order = order


# This is an ugly hack
# The remotenames extension removes the --rebase flag from pull so that the
# upstream rebase won't rebase to the wrong place. However, we want to allow
# the user to specify an explicit destination, but still abort if the user
# specifies dest without update or rebase. Conveniently, _getrebasedest is
# called before the --rebase flag is stripped from the opts. We will save it
# when _getrebasedest is called, then look it up in the pull command to do the
# right thing.
rebaseflag = False
rebasedest = None


def _getrebasedest(orig, repo, opts):
    """Use the manually specified destination over the tracking destination"""
    global rebaseflag, rebasedest
    rebaseflag = opts.get("rebase")
    origdest = orig(repo, opts)
    dest = opts.get("dest")
    if not dest:
        dest = origdest
    rebasedest = dest
    return dest


def pull(orig, ui, repo, *args, **opts):
    """pull --rebase/--update are problematic without an explicit destination"""
    try:
        rebasemodule = extensions.find("rebase")
    except KeyError:
        rebasemodule = None

    rebase = opts.get("rebase")
    update = opts.get("update")
    isrebase = rebase or rebaseflag
    # Only use from the global rebasedest if _getrebasedest was called.  If the
    # user isn't using remotenames, then rebasedest isn't set.
    if rebaseflag:
        dest = rebasedest
    else:
        dest = opts.get("dest")

    if (isrebase or update) and not dest:
        dest = ui.config("tweakdefaults", "defaultdest")

    if isrebase and update:
        mess = _("specify either rebase or update, not both")
        raise error.Abort(mess)

    if dest and not (isrebase or update):
        mess = _("only specify a destination if rebasing or updating")
        raise error.Abort(mess)

    if (isrebase or update) and not dest:
        mess = None
        if isrebase and repo._activebookmark:
            mess = ui.config("tweakdefaults", "bmnodestmsg")
            hint = ui.config("tweakdefaults", "bmnodesthint")
        elif isrebase:
            mess = ui.config("tweakdefaults", "nodestmsg")
            hint = ui.config("tweakdefaults", "nodesthint")
        elif not opts.get("bookmark") and not opts.get("rev"):  # update
            mess = _("you must specify a destination for the update")
            hint = _("use `@prog@ pull --update --dest <destination>`")
        if mess is not None:
            raise error.Abort(mess, hint=hint)

    if "rebase" in opts:
        del opts["rebase"]
        tool = opts.pop("tool", "")
    if "update" in opts and dest:
        del opts["update"]
    if "dest" in opts and dest:
        del opts["dest"]

    ret = orig(ui, repo, *args, **opts)

    # NB: we use rebase and not isrebase on the next line because
    # remotenames may have already handled the rebase.
    if dest and rebase:
        ret = ret or rebaseorfastforward(
            rebasemodule.rebase, ui, repo, dest=dest, tool=tool
        )
    if dest and update:
        ret = ret or commands.update(ui, repo, node=dest, check=True)

    return ret


def rebaseorfastforward(orig, ui, repo, dest, **args):
    """Wrapper for rebasemodule.rebase that fast-forwards the working directory
    and any active bookmark to the rebase destination if there is actually
    nothing to rebase.
    """
    prev = repo["."]
    destrev = scmutil.revsingle(repo, dest)
    common = destrev.ancestor(prev)
    if prev == common and destrev != prev:
        result = hg.update(repo, destrev.node())
        if repo._activebookmark:
            with repo.wlock():
                bookmarks.update(repo, [prev.node()], destrev.node())
        ui.status(_("nothing to rebase - fast-forwarded to %s\n") % dest)
        return result
    return orig(ui, repo, dest=dest, **args)


def pullrebaseffwd(orig, rebasefunc, ui, repo, source: str = "default", **opts):
    # The remotenames module also wraps "pull --rebase", and if it is active, it
    # is the module that actually performs the rebase.  If it is rebasing, we
    # need to wrap the rebasemodule.rebase function that it calls to replace it
    # with our rebaseorfastforward method.
    rebasing = "rebase" in opts
    if rebasing:
        rebasemodule = extensions.find("rebase")
        if rebasemodule:
            wrapfunction(rebasemodule, "rebase", rebaseorfastforward)
    ret = orig(rebasefunc, ui, repo, source, **opts)
    # pyre-fixme[61]: `rebasemodule` is undefined, or not always defined.
    if rebasing and rebasemodule:
        # pyre-fixme[61]: `rebasemodule` is undefined, or not always defined.
        extensions.unwrapfunction(rebasemodule, "rebase", rebaseorfastforward)
    return ret


def commitcmd(orig, ui, repo, *pats, **opts):
    if (
        opts.get("amend")
        and not opts.get("date")
        and not opts.get("to")
        and not ui.configbool("tweakdefaults", "amendkeepdate")
    ):
        opts["date"] = currentdate()
    return orig(ui, repo, *pats, **opts)


def wrapblame() -> None:
    entry = wrapcommand(commands.table, "annotate", blame)
    options = entry[1]
    options.append(("p", "phabdiff", None, _("list phabricator diff id")))

    # revision number is no longer default
    nind = next((i for i, o in enumerate(options) if o[0] == "n"), -1)
    if nind != -1:
        options[nind] = ("n", "number", None, _("list the revision number"))
    # changeset is default now
    cind = next((i for i, o in enumerate(options) if o[0] == "c"), -1)
    if cind != -1:
        options[cind] = ("c", "changeset", None, _("list the changeset (default)"))


def blame(orig, ui, repo, *pats, **opts):
    @templater.templatefunc("blame_phabdiffid")
    def phabdiff(context, mapping, args):
        """Fetch the Phab Diff Id from the node in mapping"""
        res = ""
        try:
            d = repo[mapping["rev"]].description()
            pat = r"https://.*/(D\d+)"
            m = re.search(pat, d)
            res = m.group(1) if m else ""
        except Exception:
            pass
        return res

    if not ui.plain():
        # changeset is the new default
        if all(
            not opts.get(f)
            for f in ["changeset", "number", "phabdiff", "user", "date", "file"]
        ):
            opts["changeset"] = True

    # without --phabdiff or with -T, use the default formatter
    if not opts.get("phabdiff") or opts.get("template"):
        return orig(ui, repo, *pats, **opts)

    # to show the --phabdiff column, we want to modify "opmap" in
    # commands.annotate - not doable directly so let's use templates to
    # workaround.
    ptmpl = [""]

    def append(t, sep=" "):
        if ptmpl[0]:
            ptmpl[0] += sep
        ptmpl[0] += t

    if opts.get("user"):
        append("{pad(user|emailuser, 13, ' ', True)}")
    if opts.get("number"):
        width = len(str(len(repo)))
        append("{pad(rev, %d)}" % width)
    if opts.get("changeset"):
        append("{short(node)}")
    if opts.get("phabdiff"):
        opts["number"] = True  # makes mapping['rev'] available in phabdiff
        append("{pad(blame_phabdiffid(), 9, ' ', True)}")
    if opts.get("date"):
        if ui.quiet:
            append("{pad(date|shortdate, 10)}")
        else:
            append("{pad(date|rfc822date, 12)}")
    if opts.get("file"):
        append("{file}")
    if opts.get("line_number"):
        append("{pad(line_number, 5, ' ', True)}", sep=":")
    opts["template"] = (
        '{lines % "{label(\\"blame.age.{age_bucket}\\", \\"'
        + ptmpl[0]
        + ': \\")}{line}"}'
    )
    return orig(ui, repo, *pats, **opts)


def _analyzewrapper(orig, x, ui):
    """Wraps analyzer to detect the use of colons in the revisions"""
    result = orig(x)

    warn = ui.configbool("tweakdefaults", "singlecolonwarn")
    abort = ui.configbool("tweakdefaults", "singlecolonabort")
    enabled = warn or abort

    # The last condition is added so that warnings are not shown if
    # hg log --follow is invoked w/o arguments
    if (
        enabled
        and isinstance(x, tuple)
        and (x[0] in ("range", "rangepre", "rangepost"))
        and x != ("rangepre", ("symbol", "."))
    ):
        if not util.istest():
            ui.deprecate("single-colon-revset", "':' is deprecated in revsets")
        msg = ui.config("tweakdefaults", "singlecolonmsg")
        if abort:
            raise error.Abort("%s" % msg)
        if warn:
            ui.warn(_("warning: %s\n") % msg)

    return result


def _rebase(orig, ui, repo, *pats, **opts):
    if not opts.get("date") and not ui.configbool("tweakdefaults", "rebasekeepdate"):
        opts["date"] = currentdate()

    if opts.get("continue") or opts.get("abort") or opts.get("restack"):
        return orig(ui, repo, *pats, **opts)

    # 'hg rebase' w/o args should do nothing
    if not opts.get("dest"):
        raise error.Abort("you must specify a destination (-d) for the rebase")

    # 'hg rebase' can fast-forward bookmark
    prev = repo["."]

    # Only fast-forward the bookmark if no source nodes were explicitly
    # specified.
    if not (opts.get("base") or opts.get("source") or opts.get("rev")):
        dest = scmutil.revsingle(repo, opts.get("dest"))
        common = dest.ancestor(prev)
        if prev == common:
            activebookmark = repo._activebookmark
            result = hg.updatetotally(ui, repo, dest.node(), activebookmark)
            if activebookmark:
                with repo.wlock():
                    bookmarks.update(repo, [prev.node()], dest.node())
            return result

    return orig(ui, repo, *pats, **opts)


# set of commands which define their own formatter and prints the hash changes
formattercommands: Set[str] = set(["fold"])


def cleanupnodeswrapper(orig, repo, mapping, operation, *args, **kwargs):
    if (
        repo.ui.configbool("tweakdefaults", "showupdated")
        and operation not in formattercommands
    ):
        maxoutput = 10
        try:
            oldnodes = list(mapping.keys())
        except AttributeError:
            # "mapping" is not always a dictionary.
            pass
        else:
            for i in range(0, min(len(oldnodes), maxoutput)):
                oldnode = oldnodes[i]
                newnodes = mapping[oldnode]
                _printupdatednode(repo, oldnode, newnodes)
            if len(oldnodes) > maxoutput + 1:
                repo.ui.status(_("...\n"))
                lastoldnode = oldnodes[-1]
                lastnewnodes = mapping[lastoldnode]
                _printupdatednode(repo, lastoldnode, lastnewnodes)
    return orig(repo, mapping, operation, *args, **kwargs)


def _printupdatednode(repo, oldnode, newnodes: List) -> None:
    # oldnode was not updated if newnodes is an iterable
    if len(newnodes) == 1:
        newnode = newnodes[0]
        firstline = encoding.trim(repo[newnode].description().split("\n")[0], 50, "...")
        repo.ui.status(
            _('%s -> %s "%s"\n') % (short(oldnode), short(newnode), firstline)
        )


def _computeobsoletenotrebasedwrapper(orig, repo, rebaseobsrevs, dest):
    """Wrapper for _computeobsoletenotrebased from rebase extensions

    Unlike upstream rebase, we don't want to skip purely pruned commits.
    We also want to explain why some particular commit was skipped."""
    res = orig(repo, rebaseobsrevs, dest)
    obsoletenotrebased = res[0]
    for key in obsoletenotrebased.keys():
        if obsoletenotrebased[key] is None:
            # key => None is a sign of a pruned commit
            del obsoletenotrebased[key]
    return res


def _checkobsrebasewrapper(orig, repo, ui, *args) -> None:
    overrides = {("experimental", "evolution.allowdivergence"): True}
    with repo.ui.configoverride(overrides, "tweakdefaults"):
        orig(repo, ui, *args)


def currentdate() -> str:
    return "%d %d" % util.makedate(time.time())


def graftcmd(orig, ui, repo, *revs, **opts):
    if not opts.get("date") and not ui.configbool("tweakdefaults", "graftkeepdate"):
        opts["date"] = currentdate()
    return orig(ui, repo, *revs, **opts)


def absorbcmd(orig, ui, repo, *pats, **opts):
    if not opts.get("date") and not ui.configbool("tweakdefaults", "absorbkeepdate"):
        opts["date"] = currentdate()
    return orig(ui, repo, *pats, **opts)


def amendcmd(orig, ui, repo, *pats, **opts):
    if (
        not opts.get("date")
        and not opts.get("to")
        and not ui.configbool("tweakdefaults", "amendkeepdate")
    ):
        opts["date"] = currentdate()
    return orig(ui, repo, *pats, **opts)


def histeditcommitfuncfor(orig, repo, src):
    origcommitfunc = orig(repo, src)

    def commitfunc(**kwargs):
        if not repo.ui.configbool("tweakdefaults", "histeditkeepdate"):
            kwargs["date"] = util.makedate(time.time())
        origcommitfunc(**kwargs)

    return commitfunc


def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get("rev") and not opts.get("all"):
        opts["follow"] = True

    return orig(ui, repo, *pats, **opts)


def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Make status relative by default for interactive usage
    """
    rootrel = opts.get("root_relative")
    if rootrel:
        del opts["root_relative"]
        if pats:
            # Ugh. So, if people pass a pattern and --root-relative,
            # they will get pattern behavior and not any root-relative paths,
            # because that's how hg status works. It's non-trivial to fixup
            # either all the patterns or all the output, so we just raise
            # an exception instead.
            message = _("--root-relative not supported with patterns")
            hint = _("run from the repo root instead")
            raise error.Abort(message, hint=hint)
    elif ui.plain():
        pass
    # Here's an ugly hack! If users are passing "re:" to make status relative,
    # hgwatchman will never refresh the full state and status will become and
    # remain slow after a restart or 24 hours. Here, we check for this and
    # replace 're:' with '' which has the same semantic effect but works for
    # hgwatchman (because match.always() == True), if and only if 're:' is the
    # only pattern passed.
    #
    # Also set pats to [''] if pats is empty because that makes status relative.
    elif not pats or (len(pats) == 1 and pats[0] == "re:"):
        pats = [""]

    with ui.configoverride(
        {("commands", "status.relative"): "false"}
    ) if rootrel else util.nullcontextmanager():
        return orig(ui, repo, *pats, **opts)


def unfilteredcmd(orig, *args, **opts):
    # use unfiltered repo for performance
    #
    # find the "repo" arg and change it to the unfiltered version.
    # "repo" could in different location, for example:
    #   args = [ui, repo, ...] for commands.bookmark
    #   args = [orig, ui, repo, ...] for remotenames.exbookmarks
    for i in [1, 2]:
        if len(args) > i and util.safehasattr(args[i], "unfiltered"):
            args = list(args)
            args = tuple(args)
    return orig(*args, **opts)


def diffcmd(orig, ui, repo, *args, **opts):
    if not opts.get("per_file_stat_json"):
        return orig(ui, repo, *args, **opts)

    ui.pushbuffer()
    res = orig(ui, repo, *args, **opts)
    buffer = ui.popbufferbytes()
    difflines = util.iterlines([buffer])
    diffstat = patch.diffstatdata(difflines)
    output = {}
    for filename, adds, removes, isbinary in diffstat:
        output[filename] = {"adds": adds, "removes": removes, "isbinary": isbinary}
    ui.write("%s\n" % (json.dumps(output, sort_keys=True)))
    return res


def _fixpager(ui) -> None:
    # users may mistakenly set PAGER=less, which will affect "pager.pager".
    # raw "less" does not support colors and is not friendly, add "-FRQX"
    # automatically.
    if ui.config("pager", "pager", "").strip() == "less":
        ui.setconfig("pager", "pager", "less -FRQX")


def get_winpopen4(pipei_bufsize):
    def winpopen4(orig, cmd, env=None, newlines=False, bufsize=-1):
        """Same as util.popen4, but manually creates an input pipe with a
        larger than default buffer"""
        import msvcrt

        if sys.version_info[0] < 3:
            import _subprocess

            handles = _subprocess.CreatePipe(None, pipei_bufsize)
            rfd, wfd = [msvcrt.open_osfhandle(h, 0) for h in handles]
        else:
            import _winapi

            handles = _winapi.CreatePipe(None, pipei_bufsize)
            rfd, wfd = [msvcrt.open_osfhandle(h, 0) for h in handles]
            handles = [subprocess.Handle(h) for h in handles]

        handles[0].Detach()
        handles[1].Detach()
        p = subprocess.Popen(
            cmd,
            shell=True,
            bufsize=bufsize,
            close_fds=False,
            stdin=rfd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=newlines,
            env=env,
        )
        p.stdin = util.fdopen(wfd, "wb", bufsize)
        return p.stdin, p.stdout, p.stderr, p

    return winpopen4

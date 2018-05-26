# tweakdefaults.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""user friendly defaults

This extension changes defaults to be more user friendly.

  hg bookmarks  always use unfiltered repo (--hidden)
  hg log        always follows history (-f)
  hg rebase     aborts without arguments
  hg update     aborts without arguments
  hg branch     aborts and encourages use of bookmarks
  hg grep       greps the working directory instead of history
  hg histgrep   renamed from grep

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

    # whether to allow or disable some commands
    allowbranch = True
    allowfullrepohistgrep = False
    allowmerge = True
    allowrollback = True
    allowtags = True

    # change rebase exit from 1 to 0 if nothing is rebased
    nooprebase = True

    # whether to show a warning or abort on some deprecated usages
    singlecolonwarn = False
    singlecolonabort = False

    # educational messages
    bmnodesthint = ''
    bmnodestmsg = ''
    branchmessage = ''
    branchesmessage = ''
    mergemessage = ''
    nodesthint = ''
    nodestmsg = ''
    rollbackhint = ''
    rollbackmessage = ''
    singlecolonmsg = ''
    tagmessage = ''
    tagsmessage = ''

    # output new hashes when nodes get updated
    showupdated = False
"""
from __future__ import absolute_import

import inspect
import json
import os
import re
import shlex
import stat
import subprocess
import time

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    encoding,
    error,
    extensions,
    hg,
    hintutil,
    obsolete,
    patch,
    pycompat,
    registrar,
    revsetlang,
    scmutil,
    templatekw,
    templater,
    util,
)
from mercurial.i18n import _
from mercurial.node import short

from . import rebase


wrapcommand = extensions.wrapcommand
wrapfunction = extensions.wrapfunction

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"

globaldata = "globaldata"
createmarkersoperation = "createmarkersoperation"

logopts = [("", "all", None, _("shows all changesets in the repo"))]

configtable = {}
configitem = registrar.configitem(configtable)

configitem("grep", "command", default="grep")
configitem(globaldata, createmarkersoperation, default=None)

configitem("tweakdefaults", "singlecolonabort", default=False)
configitem("tweakdefaults", "singlecolonwarn", default=False)
configitem("tweakdefaults", "showupdated", default=False)
configitem("tweakdefaults", "nooprebase", default=True)

configitem("tweakdefaults", "amendkeepdate", default=False)
configitem("tweakdefaults", "graftkeepdate", default=False)
configitem("tweakdefaults", "histeditkeepdate", default=False)
configitem("tweakdefaults", "rebasekeepdate", default=False)

configitem("tweakdefaults", "allowbranch", default=True)
configitem("tweakdefaults", "allowfullrepohistgrep", default=False)
configitem("tweakdefaults", "allowmerge", default=True)
configitem("tweakdefaults", "allowrollback", default=True)
configitem("tweakdefaults", "allowtags", default=True)

rebasemsg = _(
    "you must use a bookmark with tracking "
    "or manually specify a destination for the rebase"
)
configitem(
    "tweakdefaults",
    "bmnodesthint",
    default=_(
        "set up tracking with `hg book -t <destination>` "
        "or manually supply --dest / -d"
    ),
)
configitem("tweakdefaults", "bmnodestmsg", default=rebasemsg)
configitem(
    "tweakdefaults",
    "branchmessage",
    default=_("new named branches are disabled in this repository"),
)
configitem("tweakdefaults", "branchesmessage", default=None)
configitem(
    "tweakdefaults",
    "mergemessage",
    default=_("merging is not supported for this repository"),
)
configitem(
    "tweakdefaults",
    "nodesthint",
    default=_(
        "set up tracking with `hg book <name> -t <destination>` "
        "or manually supply --dest / -d"
    ),
)
configitem("tweakdefaults", "nodestmsg", default=rebasemsg)
configitem(
    "tweakdefaults", "rollbackmessage", default=_("the use of rollback is disabled")
)
configitem("tweakdefaults", "rollbackhint", default=None)
configitem("tweakdefaults", "singlecolonmsg", default=_("use of ':' is deprecated"))
configitem(
    "tweakdefaults", "tagmessage", default=_("new tags are disabled in this repository")
)
configitem("tweakdefaults", "tagsmessage", default="")


def uisetup(ui):
    tweakorder()
    # if we want to replace command's docstring (not just add stuff to it),
    # we need to do it in uisetup, not extsetup
    commands.table["^annotate|blame"][0].__doc__ = blame.__doc__


def extsetup(ui):
    entry = wrapcommand(commands.table, "update", update)
    options = entry[1]
    # try to put in alphabetical order
    options.insert(3, ("", "inactive", None, _("update without activating bookmarks")))
    wrapblame()

    entry = wrapcommand(commands.table, "commit", commitcmd)
    options = entry[1]
    options.insert(
        9, ("M", "reuse-message", "", _("reuse commit message from REV"), _("REV"))
    )
    opawarerebase = markermetadatawritingcommand(ui, _rebase, "rebase")
    wrapcommand(rebase.cmdtable, "rebase", opawarerebase)
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

    entry = wrapcommand(commands.table, "branch", branchcmd)
    options = entry[1]
    options.append(("", "new", None, _("allow branch creation")))
    wrapcommand(commands.table, "branches", branchescmd)

    wrapcommand(commands.table, "merge", mergecmd)

    entry = wrapcommand(commands.table, "status", statuscmd)
    options = entry[1]
    options.append(("", "root-relative", None, _("show status relative to root")))

    wrapcommand(commands.table, "rollback", rollbackcmd)

    wrapcommand(commands.table, "tag", tagcmd)
    wrapcommand(commands.table, "tags", tagscmd)
    wrapcommand(commands.table, "graft", graftcmd)
    try:
        fbamendmodule = extensions.find("fbamend")
        opawareamend = markermetadatawritingcommand(ui, amendcmd, "amend")
        wrapcommand(fbamendmodule.cmdtable, "amend", opawareamend)
    except KeyError:
        pass
    try:
        histeditmodule = extensions.find("histedit")
        wrapfunction(histeditmodule, "commitfuncfor", histeditcommitfuncfor)
    except KeyError:
        pass

    # wrapped createmarkers knows how to write operation-aware
    # metadata (e.g. 'amend', 'rebase' and so forth)
    wrapfunction(obsolete, "createmarkers", _createmarkers)

    # bookmark -D is an alias to strip -B
    entry = wrapcommand(commands.table, "bookmarks", bookmarkcmd)
    entry[1].insert(
        3, ("D", "strip", None, _("like --delete but also strip changesets"))
    )

    # wrap bookmarks after remotenames
    def afterloaded(loaded):
        if loaded:
            # remotenames is loaded, wrap its wrapper directly
            remotenames = extensions.find("remotenames")
            wrapfunction(remotenames, "exbookmarks", unfilteredcmd)
            wrapfunction(remotenames, "expullcmd", pullrebaseffwd)
        else:
            # otherwise wrap the bookmarks command
            wrapcommand(commands.table, "bookmarks", unfilteredcmd)

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

    # Tweak Behavior
    tweakbehaviors(ui)
    _fixpager(ui)

    # Change manifest template output
    templatekw.defaulttempl["manifest"] = "{node}"


def reposetup(ui, repo):
    _fixpager(ui)
    # Allow uncommit on dirty working directory
    repo.ui.setconfig("experimental", "uncommitondirtywdir", True)
    # Allow unbundling of pushvars on server
    repo.ui.setconfig("push", "pushvars.server", True)


def tweakorder():
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
        if isrebase and bmactive(repo):
            mess = ui.config("tweakdefaults", "bmnodestmsg")
            hint = ui.config("tweakdefaults", "bmnodesthint")
        elif isrebase:
            mess = ui.config("tweakdefaults", "nodestmsg")
            hint = ui.config("tweakdefaults", "nodesthint")
        else:  # update
            mess = _("you must specify a destination for the update")
            hint = _("use `hg pull --update --dest <destination>`")
        raise error.Abort(mess, hint=hint)

    if "rebase" in opts:
        del opts["rebase"]
        tool = opts.pop("tool", "")
    if "update" in opts:
        del opts["update"]
    if "dest" in opts:
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
        if bmactive(repo):
            with repo.wlock():
                bookmarks.update(repo, [prev.node()], destrev.node())
        ui.status(_("nothing to rebase - fast-forwarded to %s\n") % dest)
        return result
    return orig(ui, repo, dest=dest, **args)


def pullrebaseffwd(orig, rebasefunc, ui, repo, source="default", **opts):
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
    if rebasing and rebasemodule:
        extensions.unwrapfunction(rebasemodule, "rebase", rebaseorfastforward)
    return ret


def tweakbehaviors(ui):
    """Tweak Behaviors

    Right now this only tweaks the rebase behavior such that the default
    exit status code for a noop rebase becomes 0 instead of 1.

    In future we may add or modify other behaviours here.
    """

    # noop rebase returns 0
    def _nothingtorebase(orig, *args, **kwargs):
        return 0

    if ui.configbool("tweakdefaults", "nooprebase"):
        try:
            rebase = extensions.find("rebase")
            extensions.wrapfunction(rebase, "_nothingtorebase", _nothingtorebase)
        except (KeyError, AttributeError):
            pass


def commitcmd(orig, ui, repo, *pats, **opts):
    if (
        opts.get("amend")
        and not opts.get("date")
        and not opts.get("to")
        and not ui.configbool("tweakdefaults", "amendkeepdate")
    ):
        opts["date"] = currentdate()

    rev = opts.get("reuse_message")
    if rev:
        invalidargs = ["message", "logfile"]
        currentinvalidargs = [ia for ia in invalidargs if opts.get(ia)]
        if currentinvalidargs:
            raise error.Abort(
                _("--reuse-message and --%s are " "mutually exclusive")
                % (currentinvalidargs[0])
            )

    if rev:
        opts["message"] = scmutil.revsingle(repo, rev).description()

    return orig(ui, repo, *pats, **opts)


def update(orig, ui, repo, node=None, rev=None, **kwargs):
    # 'hg update' should do nothing
    #  Note if you want to change this:
    # --inactive requires arg checkout of
    # updatetotally not to be none
    if not node and not rev and not kwargs["date"]:
        raise error.Abort(
            "You must specify a destination to update to,"
            + ' for example "hg update master".',
            hint="If you're trying to move a bookmark forward, try "
            + '"hg rebase -d <destination>".',
        )

    # Doesn't activate inactive bookmarks with this flag
    # In order to avoid submitting to upstream:
    #   assumes checkout not to be none
    #   assumes whitespace to be illegal bookmark char
    # Wrapping bookmarks' active with pass will
    # give you the same behavior without the assumptions
    # but will print wrong output
    inactive = kwargs.pop("inactive")
    if inactive:
        wrapfunction(hg, "updatetotally", _wrapupdatetotally)

    result = orig(ui, repo, node=node, rev=rev, **kwargs)

    if inactive:
        extensions.unwrapfunction(hg, "updatetotally", _wrapupdatetotally)

    # If the command succeed a message for 'hg update .^' will appear
    # suggesting to use hg prev
    if node == ".^":
        hintutil.trigger("update-prev")

    return result


def _wrapupdatetotally(orig, ui, repo, checkout, brev, *args, **kwargs):
    # set brev to invalidbookmark to prevent bookmark update
    invalidbookmark = " "
    assert checkout is not None
    assert invalidbookmark not in repo._bookmarks
    # assert invalidbookmark is not None
    orig(ui, repo, checkout, invalidbookmark, *args, **kwargs)


def wrapblame():
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
    """show changeset information by line for each file

    List changes in files, showing the changeset responsible for
    each line.

    This command is useful for discovering when a change was made and
    by whom.

    If you include -n, changeset gets replaced by revision id, unless
    you also include -c, in which case both are shown. -p on the other
    hand always adds Phabricator Diff Id, not replacing anything with it.

    Without the -a/--text option, annotate will avoid processing files
    it detects as binary. With -a, annotate will annotate the file
    anyway, although the results will probably be neither useful
    nor desirable.

    Returns 0 on success.
    """

    @templater.templatefunc("blame_phabdiffid")
    def phabdiff(context, mapping, args):
        """Fetch the Phab Diff Id from the node in mapping"""
        res = " " * 8
        try:
            d = repo[mapping["rev"]].description()
            pat = "https://.*/(D\d+)"
            m = re.search(pat, d)
            res = m.group(1) if m else ""
        except Exception:
            pass
        return res

    if ui.plain():
        return orig(ui, repo, *pats, **opts)

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
        append("{pad(blame_phabdiffid(), 8)}")
    if opts.get("date"):
        if ui.quiet:
            append("{pad(date|shortdate, 10)}")
        else:
            append("{pad(date|rfc822date, 12)}")
    if opts.get("file"):
        append("{file}")
    if opts.get("line_number"):
        append("{pad(line_number, 5, ' ', True)}", sep=":")
    opts["template"] = '{lines % "' + ptmpl[0] + ': {line}"}'
    return orig(ui, repo, *pats, **opts)


@command("histgrep", commands.table["grep"][1], commands.table["grep"][2])
def histgrep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions

    Search revisions of files for a regular expression.

    The command used to be hg grep.

    This command behaves differently than Unix grep. It only accepts
    Python/Perl regexps. It searches repository history, not the working
    directory. It always prints the revision number in which a match appears.

    By default, grep only prints output for the first revision of a file in
    which it finds a match. To get it to print every revision that contains a
    change in match status ("-" for a match that becomes a non-match, or "+"
    for a non-match that becomes a match), use the --all flag.

    Returns 0 if a match is found, 1 otherwise."""
    if not pats and not ui.configbool("tweakdefaults", "allowfullrepohistgrep"):
        m = _("can't run histgrep on the whole repo, please provide filenames")
        h = _("this is disabled to avoid very slow greps over the whole repo")
        raise error.Abort(m, hint=h)

    return commands.grep(ui, repo, pattern, *pats, **opts)


del commands.table["grep"]


@command(
    "grep",
    [
        ("A", "after-context", "", "print NUM lines of trailing context", "NUM"),
        ("B", "before-context", "", "print NUM lines of leading context", "NUM"),
        ("C", "context", "", "print NUM lines of output context", "NUM"),
        ("i", "ignore-case", None, "ignore case when matching"),
        ("l", "files-with-matches", None, "print only filenames that match"),
        ("n", "line-number", None, "print matching line numbers"),
        ("V", "invert-match", None, "select non-matching lines"),
        ("w", "word-regexp", None, "match whole words only"),
        ("E", "extended-regexp", None, "use POSIX extended regexps"),
        ("F", "fixed-strings", None, "interpret pattern as fixed string"),
        ("P", "perl-regexp", None, "use Perl-compatible regexps"),
        (
            "I",
            "include",
            [],
            _("include names matching the given patterns"),
            _("PATTERN"),
        ),
        (
            "X",
            "exclude",
            [],
            _("exclude names matching the given patterns"),
            _("PATTERN"),
        ),
    ],
    "[OPTION]... PATTERN [FILE]...",
    inferrepo=True,
)
def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in tracked files in the working directory

    The default regexp style is POSIX basic regexps. If no FILE parameters are
    passed in, the current directory and its subdirectories will be searched.

    For the old 'hg grep', see 'histgrep'."""

    grepcommandstr = ui.config("grep", "command")
    # Use shlex.split() to split up grepcommandstr into multiple arguments.
    # this allows users to specify a command plus arguments (e.g., "grep -i").
    # We don't use a real shell to execute this, which ensures we won't do
    # bad stuff if their command includes redirects, semicolons, or other
    # special characters etc.
    cmd = (
        ["xargs", "-0"]
        + shlex.split(grepcommandstr)
        + [
            "--no-messages",
            "--binary-files=without-match",
            "--with-filename",
            "--regexp=" + pattern,
        ]
    )

    if opts.get("after_context"):
        cmd.append("-A")
        cmd.append(opts.get("after_context"))
    if opts.get("before_context"):
        cmd.append("-B")
        cmd.append(opts.get("before_context"))
    if opts.get("context"):
        cmd.append("-C")
        cmd.append(opts.get("context"))
    if opts.get("ignore_case"):
        cmd.append("-i")
    if opts.get("files_with_matches"):
        cmd.append("-l")
    if opts.get("line_number"):
        cmd.append("-n")
    if opts.get("invert_match"):
        cmd.append("-v")
    if opts.get("word_regexp"):
        cmd.append("-w")
    if opts.get("extended_regexp"):
        cmd.append("-E")
    if opts.get("fixed_strings"):
        cmd.append("-F")
    if opts.get("perl_regexp"):
        cmd.append("-P")

    # color support, using the color extension
    colormode = getattr(ui, "_colormode", "")
    if colormode == "ansi":
        cmd.append("--color=always")

    # Copy match specific options
    match_opts = {}
    for k in ("include", "exclude"):
        if k in opts:
            match_opts[k] = opts.get(k)

    wctx = repo[None]
    if not pats:
        # Search everything in the current directory
        m = scmutil.match(wctx, ["."], match_opts)
    else:
        # Search using the specified patterns
        m = scmutil.match(wctx, pats, match_opts)

    # Add '--' to make sure grep recognizes all remaining arguments
    # (passed in by xargs) as filenames.
    cmd.append("--")
    ui.pager("grep")
    p = subprocess.Popen(
        cmd, bufsize=-1, close_fds=util.closefds, stdin=subprocess.PIPE
    )

    write = p.stdin.write
    ds = repo.dirstate
    getkind = stat.S_IFMT
    lnkkind = stat.S_IFLNK
    results = ds.walk(m, subrepos=[], unknown=False, ignored=False)
    for f in sorted(results.keys()):
        st = results[f]
        # skip symlinks and removed files
        if st is None or getkind(st.st_mode) == lnkkind:
            continue
        write(m.rel(f) + "\0")

    p.stdin.close()
    return p.wait()


def markermetadatawritingcommand(ui, origcmd, operationame):
    """Wrap origcmd in a context where globaldata config contains
    the name of current operation so that any function up the call
    stack can query for this value:
        `repo.ui.config(globaldata, createmarkersoperation)`

    In particular, we want `obsolete.createmarkers` to know whether
    top-level scenario is amend, rebase or something else so that
    it can write these values into marker metadata.
    """
    origargs = inspect.getargspec(origcmd)
    try:
        repo_index = origargs.args.index("repo")
    except ValueError:
        ui.warn(_("cannot wrap a command that does not have repo argument"))
        return origcmd

    def cmd(*args, **kwargs):
        repo = args[repo_index]
        overrides = {(globaldata, createmarkersoperation): operationame}
        with repo.ui.configoverride(overrides, "tweakdefaults"):
            return origcmd(*args, **kwargs)

    return cmd


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
        msg = ui.config("tweakdefaults", "singlecolonmsg")
        if abort:
            raise error.Abort("%s" % msg)
        if warn:
            ui.warn(_("warning: %s\n") % msg)

    return result


def _rebase(orig, ui, repo, **opts):
    if not opts.get("date") and not ui.configbool("tweakdefaults", "rebasekeepdate"):
        opts["date"] = currentdate()

    if opts.get("continue") or opts.get("abort") or opts.get("restack"):
        return orig(ui, repo, **opts)

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
            result = hg.update(repo, dest.node())
            if bmactive(repo):
                with repo.wlock():
                    bookmarks.update(repo, [prev.node()], dest.node())
            return result

    return orig(ui, repo, **opts)


# set of commands which define their own formatter and prints the hash changes
formattercommands = set(["fold"])


def cleanupnodeswrapper(orig, repo, mapping, operation, *args, **kwargs):
    if (
        repo.ui.configbool("tweakdefaults", "showupdated")
        and operation not in formattercommands
    ):
        maxoutput = 10
        oldnodes = sorted(mapping.keys())
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


def _printupdatednode(repo, oldnode, newnodes):
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


def _checkobsrebasewrapper(orig, repo, ui, *args):
    overrides = {}
    try:
        extensions.find("inhibit")
        # if inhibit is enabled, allow divergence
        overrides[("experimental", "evolution.allowdivergence")] = True
    except KeyError:
        pass
    with repo.ui.configoverride(overrides, "tweakdefaults"):
        orig(repo, ui, *args)


def currentdate():
    return "%d %d" % util.makedate(time.time())


def graftcmd(orig, ui, repo, *revs, **opts):
    if not opts.get("date") and not ui.configbool("tweakdefaults", "graftkeepdate"):
        opts["date"] = currentdate()
    return orig(ui, repo, *revs, **opts)


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


def branchcmd(orig, ui, repo, label=None, **opts):
    message = ui.config("tweakdefaults", "branchmessage")
    enabled = ui.configbool("tweakdefaults", "allowbranch")
    if (enabled and opts.get("new")) or label is None:
        if "new" in opts:
            del opts["new"]
        return orig(ui, repo, label, **opts)
    elif enabled:
        raise error.Abort(
            _("do not use branches; use bookmarks instead"),
            hint=_("use --new if you are certain you want a branch"),
        )
    else:
        raise error.Abort(message)


def branchescmd(orig, ui, repo, active, closed, **opts):
    message = ui.config("tweakdefaults", "branchesmessage")
    if message:
        ui.warn(message + "\n")
    return orig(ui, repo, active, closed, **opts)


def mergecmd(orig, ui, repo, node=None, **opts):
    """
    Allowing to disable merges
    """
    if ui.configbool("tweakdefaults", "allowmerge"):
        return orig(ui, repo, node, **opts)
    else:
        message = ui.config("tweakdefaults", "mergemessage")
        hint = ui.config("tweakdefaults", "mergehint", _("use rebase instead"))
        raise error.Abort(message, hint=hint)


def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Make status relative by default for interactive usage
    """
    if opts.get("root_relative"):
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
    elif encoding.environ.get("HGPLAIN"):  # don't break automation
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
    return orig(ui, repo, *pats, **opts)


def rollbackcmd(orig, ui, repo, **opts):
    """
    Allowing to disable the rollback command
    """
    if ui.configbool("tweakdefaults", "allowrollback"):
        return orig(ui, repo, **opts)
    else:
        message = ui.config("tweakdefaults", "rollbackmessage")
        hint = ui.config("tweakdefaults", "rollbackhint")
        raise error.Abort(message, hint=hint)


def tagcmd(orig, ui, repo, name1, *names, **opts):
    """
    Allowing to disable tags
    """
    message = ui.config("tweakdefaults", "tagmessage")
    if ui.configbool("tweakdefaults", "allowtags"):
        return orig(ui, repo, name1, *names, **opts)
    else:
        raise error.Abort(message)


def tagscmd(orig, ui, repo, **opts):
    message = ui.config("tweakdefaults", "tagsmessage")
    if message:
        ui.warn(message + "\n")
    return orig(ui, repo, **opts)


def bookmarkcmd(orig, ui, repo, *names, **opts):
    strip = opts.pop("strip")
    if not strip:
        return orig(ui, repo, *names, **opts)
    # check conflicted opts
    for name in [
        "force",
        "rev",
        "rename",
        "inactive",
        "track",
        "untrack",
        "all",
        "remote",
    ]:
        if opts.get(name):
            raise error.Abort(
                _("--strip cannot be used together with %s") % ("--%s" % name)
            )

    # call strip -B, may raise UnknownCommand
    stripfunc = cmdutil.findcmd("strip", commands.table)[1][0]
    return stripfunc(ui, repo, bookmark=names, rev=[])


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
            args[i] = args[i].unfiltered()
            args = tuple(args)
    return orig(*args, **opts)


def diffcmd(orig, ui, repo, *args, **opts):
    if not opts.get("per_file_stat_json"):
        return orig(ui, repo, *args, **opts)

    ui.pushbuffer()
    res = orig(ui, repo, *args, **opts)
    buffer = ui.popbuffer()
    difflines = util.iterlines([buffer])
    diffstat = patch.diffstatdata(difflines)
    output = {}
    for filename, adds, removes, isbinary in diffstat:
        # use special encoding that allows non-utf8 filenames
        filename = encoding.jsonescape(filename, paranoid=True)
        output[filename] = {"adds": adds, "removes": removes, "isbinary": isbinary}
    ui.write("%s\n" % (json.dumps(output, sort_keys=True)))
    return res


### bookmarks api compatibility layer ###
def bmactive(repo):
    try:
        return repo._activebookmark
    except AttributeError:
        return repo._bookmarkcurrent


def _createmarkers(
    orig, repo, relations, flag=0, date=None, metadata=None, operation=None
):
    configoperation = repo.ui.config(globaldata, createmarkersoperation)
    if configoperation is not None:
        operation = configoperation

    if operation is None:
        return orig(repo, relations, flag, date, metadata)

    # While _createmarkers in newer Mercurial does have an operation argument,
    # it is ignored unless certain configs are set. Let's just continue to set
    # it directly on the metadata for now.
    if metadata is None:
        metadata = {}
    metadata["operation"] = operation
    return orig(repo, relations, flag, date, metadata)


def _fixpager(ui):
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
        import _subprocess

        handles = _subprocess.CreatePipe(None, pipei_bufsize)
        rfd, wfd = [msvcrt.open_osfhandle(h, 0) for h in handles]
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
        p.stdin = os.fdopen(wfd, "wb", bufsize)
        return p.stdin, p.stdout, p.stderr, p

    return winpopen4

# commands.py - command processing for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import difflib
import errno
import os
import re
import shlex
import stat
import subprocess
import sys
import time

from . import (
    debug,
    debugcheckoutidentifier,
    debugmutation,
    debugstatus,
    debugstrip,
    eden,
    fs,
    uncommit,
)
from .. import (
    archival,
    bookmarks,
    bundle2,
    changegroup,
    cmdutil,
    copies,
    destutil,
    dirstateguard,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    formatter,
    graphmod,
    hbisect,
    help,
    hg,
    hintutil,
    lock as lockmod,
    merge as mergemod,
    mutation,
    obsolete,
    patch,
    phases,
    pycompat,
    rcutil,
    registrar,
    revsetlang,
    rewriteutil,
    scmutil,
    server,
    sshserver,
    streamclone,
    tags as tagsmod,
    templatekw,
    treestate,
    ui as uimod,
    util,
)
from ..i18n import _
from ..node import bin, hex, nullid, nullrev, short


release = lockmod.release

table = {}
table.update(debug.command._table)

command = registrar.command(table)
readonly = registrar.command.readonly

table.update(uncommit.cmdtable)
table.update(debugcheckoutidentifier.command._table)
table.update(debugmutation.command._table)
table.update(debugstatus.command._table)
table.update(debugstrip.command._table)
table.update(eden.command._table)
table.update(fs.command._table)

# common command options

globalopts = [
    (
        "R",
        "repository",
        "",
        _("repository root directory or name of overlay bundle file"),
        _("REPO"),
    ),
    ("", "cwd", "", _("change working directory"), _("DIR")),
    (
        "y",
        "noninteractive",
        None,
        _("do not prompt, automatically pick the first choice for all prompts"),
    ),
    ("q", "quiet", None, _("suppress output")),
    ("v", "verbose", None, _("enable additional output")),
    (
        "",
        "color",
        "",
        # i18n: 'always', 'auto', 'never', and 'debug' are keywords
        # and should not be translated
        _("when to colorize (boolean, always, auto, never, or debug)"),
        _("TYPE"),
    ),
    (
        "",
        "config",
        [],
        _("set/override config option (use 'section.name=value')"),
        _("CONFIG"),
    ),
    ("", "configfile", [], _("enables the given config file"), _("FILE")),
    ("", "debug", None, _("enable debugging output")),
    ("", "debugger", None, _("start debugger")),
    ("", "encoding", encoding.encoding, _("set the charset encoding"), _("ENCODE")),
    (
        "",
        "encodingmode",
        encoding.encodingmode,
        _("set the charset encoding mode"),
        _("MODE"),
    ),
    ("", "traceback", None, _("always print a traceback on exception")),
    ("", "time", None, _("time how long the command takes")),
    ("", "profile", None, _("print command execution profile")),
    ("", "version", None, _("output version information and exit")),
    ("h", "help", None, _("display help and exit")),
    ("", "hidden", False, _("consider hidden changesets")),
    (
        "",
        "pager",
        "auto",
        _("when to paginate (boolean, always, auto, or never)"),
        _("TYPE"),
    ),
]

dryrunopts = cmdutil.dryrunopts
remoteopts = cmdutil.remoteopts
walkopts = cmdutil.walkopts
commitopts = cmdutil.commitopts
commitopts2 = cmdutil.commitopts2
formatteropts = cmdutil.formatteropts
templateopts = cmdutil.templateopts
logopts = cmdutil.logopts
diffopts = cmdutil.diffopts
diffwsopts = cmdutil.diffwsopts
diffopts2 = cmdutil.diffopts2
mergetoolopts = cmdutil.mergetoolopts
similarityopts = cmdutil.similarityopts
debugrevlogopts = cmdutil.debugrevlogopts

# Commands start here, listed alphabetically

try:
    xrange(0)
except NameError:
    xrange = range


@command("^add", walkopts + dryrunopts, _("[OPTION]... [FILE]..."), inferrepo=True)
def add(ui, repo, *pats, **opts):
    """start tracking the specified files

    Specify files to be tracked by Mercurial. The files will be added to
    the repository at the next commit.

    To undo an add before files have been committed, use :hg:`forget`.
    To undo an add after files have been committed, use :hg:`rm`.

    If no names are given, add all files to the repository (except
    files matching ``.hgignore``).

    .. container:: verbose

       Examples:

         - New (unknown) files are added
           automatically by :hg:`add`::

             $ ls
             foo.c
             $ hg status
             ? foo.c
             $ hg add
             adding foo.c
             $ hg status
             A foo.c

         - Specific files to be added can be specified::

             $ ls
             bar.c  foo.c
             $ hg status
             ? bar.c
             ? foo.c
             $ hg add bar.c
             $ hg status
             A bar.c
             ? foo.c

    Returns 0 if all files are successfully added.
    """

    m = scmutil.match(repo[None], pats, pycompat.byteskwargs(opts))
    rejected = cmdutil.add(ui, repo, m, "", False, **opts)
    return rejected and 1 or 0


@command(
    "addremove",
    similarityopts + walkopts + dryrunopts,
    _("[OPTION]... [FILE]..."),
    inferrepo=True,
)
def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files

    Add all new files and remove all missing files from the
    repository.

    Unless names are given, new files are ignored if they match any of
    the patterns in ``.gitignore``. As with add, these changes take
    effect at the next commit.

    Use the -s/--similarity option to detect renamed files. This
    option takes a percentage between 0 (disabled) and 100 (files must
    be identical) as its parameter. With a parameter greater than 0,
    this compares every removed file with every added file and records
    those similar enough as renames. Detecting renamed files this way
    can be expensive. After using this option, :hg:`status -C` can be
    used to check which files were identified as moved or renamed. If
    not specified, -s/--similarity defaults to 100 and only renames of
    identical files are detected.

    .. container:: verbose

       Examples:

         - A number of files (bar.c and foo.c) are new,
           while foobar.c has been removed (without using :hg:`remove`)
           from the repository::

             $ ls
             bar.c foo.c
             $ hg status
             ! foobar.c
             ? bar.c
             ? foo.c
             $ hg addremove
             adding bar.c
             adding foo.c
             removing foobar.c
             $ hg status
             A bar.c
             A foo.c
             R foobar.c

         - A file foobar.c was moved to foo.c without using :hg:`rename`.
           Afterwards, it was edited slightly::

             $ ls
             foo.c
             $ hg status
             ! foobar.c
             ? foo.c
             $ hg addremove --similarity 90
             removing foobar.c
             adding foo.c
             recording removal of foobar.c as rename to foo.c (94% similar)
             $ hg status -C
             A foo.c
               foobar.c
             R foobar.c

    Returns 0 if all files are successfully added.
    """
    opts = pycompat.byteskwargs(opts)
    try:
        sim = float(opts.get("similarity") or 100)
    except ValueError:
        raise error.Abort(_("similarity must be a number"))
    if sim < 0 or sim > 100:
        raise error.Abort(_("similarity must be between 0 and 100"))
    matcher = scmutil.match(repo[None], pats, opts)
    return scmutil.addremove(repo, matcher, "", opts, similarity=sim / 100.0)


@command(
    "^annotate|blame",
    [
        ("r", "rev", "", _("annotate the specified revision"), _("REV")),
        (
            "",
            "follow",
            None,
            _("follow copies/renames and list the filename (DEPRECATED)"),
        ),
        ("", "no-follow", None, _("don't follow copies and renames")),
        ("a", "text", None, _("treat all files as text")),
        ("u", "user", None, _("list the author (long with -v)")),
        ("f", "file", None, _("list the filename")),
        ("d", "date", None, _("list the date (short with -q)")),
        ("n", "number", None, _("list the revision number (default)")),
        ("c", "changeset", None, _("list the changeset")),
        ("l", "line-number", None, _("show line number at the first appearance")),
        ("", "skip", [], _("revision to not display (EXPERIMENTAL)"), _("REV")),
    ]
    + diffwsopts
    + walkopts
    + formatteropts,
    _("[-r REV] [-f] [-a] [-u] [-d] [-n] [-c] [-l] FILE..."),
    inferrepo=True,
)
def annotate(ui, repo, *pats, **opts):
    """show changeset information by line for each file

    List changes in files, showing the revision id responsible for
    each line.

    This command is useful for discovering when a change was made and
    by whom.

    If you include --file, --user, or --date, the revision number is
    suppressed unless you also include --number.

    Without the -a/--text option, annotate will avoid processing files
    it detects as binary. With -a, annotate will annotate the file
    anyway, although the results will probably be neither useful
    nor desirable.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    if not pats:
        raise error.Abort(_("at least one filename or pattern is required"))

    if opts.get("follow"):
        # --follow is deprecated and now just an alias for -f/--file
        # to mimic the behavior of Mercurial before version 1.5
        opts["file"] = True

    ctx = scmutil.revsingle(repo, opts.get("rev"))

    rootfm = ui.formatter("annotate", opts)
    if ui.quiet:
        datefunc = util.shortdate
    else:
        datefunc = util.datestr
    if ctx.rev() is None:

        def hexfn(node):
            if node is None:
                return None
            else:
                return rootfm.hexfunc(node)

        if opts.get("changeset"):
            # omit "+" suffix which is appended to node hex
            def formatrev(rev):
                if rev is None:
                    return "%d" % ctx.p1().rev()
                else:
                    return "%d" % rev

        else:

            def formatrev(rev):
                if rev is None:
                    return "%d+" % ctx.p1().rev()
                else:
                    return "%d " % rev

        def formathex(hex):
            if hex is None:
                return "%s+" % rootfm.hexfunc(ctx.p1().node())
            else:
                return "%s " % hex

    else:
        hexfn = rootfm.hexfunc
        formatrev = formathex = pycompat.bytestr

    now = time.time()

    def agebucket(d):
        t, tz = d
        if t > now - 3600:
            return "1hour"
        day = 86400
        if t > now - day:
            return "1day"
        if t > now - 7 * day:
            return "7day"
        if t > now - 30 * day:
            return "30day"
        if t > now - 60 * day:
            return "60day"
        if t > now - 180 * day:
            return "180day"
        if t > now - 360 * day:
            return "360day"
        return "old"

    opmap = [
        ("user", " ", lambda x: x.fctx.user(), ui.shortuser),
        ("number", " ", lambda x: x.fctx.rev(), formatrev),
        ("changeset", " ", lambda x: hexfn(x.fctx.node()), formathex),
        ("date", " ", lambda x: x.fctx.date(), util.cachefunc(datefunc)),
        ("file", " ", lambda x: x.fctx.path(), str),
        ("line_number", ":", lambda x: x.lineno, str),
        ("age_bucket", "", lambda x: agebucket(x.fctx.date()), lambda x: ""),
    ]
    fieldnamemap = {"number": "rev", "changeset": "node"}

    if (
        not opts.get("user")
        and not opts.get("changeset")
        and not opts.get("date")
        and not opts.get("file")
    ):
        opts["number"] = True
    opts["agebucket"] = True

    linenumber = opts.get("line_number") is not None
    if linenumber and (not opts.get("changeset")) and (not opts.get("number")):
        raise error.Abort(_("at least one of -n/-c is required for -l"))

    ui.pager("annotate")

    if rootfm.isplain():

        def makefunc(get, fmt):
            return lambda x: fmt(get(x))

    else:

        def makefunc(get, fmt):
            return get

    funcmap = [(makefunc(get, fmt), sep) for op, sep, get, fmt in opmap if opts.get(op)]
    funcmap[0] = (funcmap[0][0], "")  # no separator in front of first column
    fields = " ".join(
        fieldnamemap.get(op, op) for op, sep, get, fmt in opmap if opts.get(op)
    )

    def bad(x, y):
        raise error.Abort("%s: %s" % (x, y))

    m = scmutil.match(ctx, pats, opts, badfn=bad)

    follow = not opts.get("no_follow")
    diffopts = patch.difffeatureopts(ui, opts, section="annotate", whitespace=True)
    skiprevs = opts.get("skip")
    if skiprevs:
        skiprevs = scmutil.revrange(repo, skiprevs)

    for abs in ctx.walk(m):
        fctx = ctx[abs]
        rootfm.startitem()
        rootfm.data(abspath=abs, path=m.rel(abs))
        if not opts.get("text") and fctx.isbinary():
            rootfm.plain(_("%s: binary file\n") % ((pats and m.rel(abs)) or abs))
            continue

        fm = rootfm.nested("lines")
        lines = fctx.annotate(
            follow=follow, linenumber=linenumber, skiprevs=skiprevs, diffopts=diffopts
        )
        if not lines:
            fm.end()
            continue
        formats = []
        pieces = []

        for f, sep in funcmap:
            l = [f(n) for n, dummy in lines]
            if fm.isplain():
                sizes = [encoding.colwidth(x) for x in l]
                ml = max(sizes)
                formats.append([sep + " " * (ml - w) + "%s" for w in sizes])
            else:
                formats.append(["%s" for x in l])
            pieces.append(l)

        agebuckets = [agebucket(x.fctx.date()) for x, dummy in lines]

        for f, p, l, a in zip(zip(*formats), zip(*pieces), lines, agebuckets):
            fm.startitem()
            sep = "* " if l[0].skip else ": "
            fm.write(fields, "".join(f) + sep, *p, label="blame.age." + a)
            fm.write("line", "%s", l[1])

        if not lines[-1][1].endswith("\n"):
            fm.plain("\n")
        fm.end()

    rootfm.end()


@command(
    "archive",
    [
        ("", "no-decode", None, _("do not pass files through decoders")),
        ("p", "prefix", "", _("directory prefix for files in archive"), _("PREFIX")),
        ("r", "rev", "", _("revision to distribute"), _("REV")),
        ("t", "type", "", _("type of distribution to create"), _("TYPE")),
    ]
    + walkopts,
    _("[OPTION]... DEST"),
)
def archive(ui, repo, dest, **opts):
    """create an unversioned archive of a repository revision

    By default, the revision used is the parent of the working
    directory; use -r/--rev to specify a different revision.

    The archive type is automatically detected based on file
    extension (to override, use -t/--type).

    .. container:: verbose

      Examples:

      - create a zip file containing the 1.0 release::

          hg archive -r 1.0 project-1.0.zip

      - create a tarball excluding .hg files::

          hg archive project.tar.gz -X ".hg*"

    Valid types are:

    :``files``: a directory full of files (default)
    :``tar``:   tar archive, uncompressed
    :``tbz2``:  tar archive, compressed using bzip2
    :``tgz``:   tar archive, compressed using gzip
    :``uzip``:  zip archive, uncompressed
    :``zip``:   zip archive, compressed using deflate

    The exact name of the destination archive or directory is given
    using a format string; see :hg:`help export` for details.

    Each member added to an archive file has a directory prefix
    prepended. Use -p/--prefix to specify a format string for the
    prefix. The default is the basename of the archive, with suffixes
    removed.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    if not ctx:
        raise error.Abort(_("no working directory: please specify a revision"))
    node = ctx.node()
    dest = cmdutil.makefilename(repo, dest, node)
    if os.path.realpath(dest) == repo.root:
        raise error.Abort(_("repository root cannot be destination"))

    kind = opts.get("type") or archival.guesskind(dest) or "files"
    prefix = opts.get("prefix")

    if dest == "-":
        if kind == "files":
            raise error.Abort(_("cannot archive plain files to stdout"))
        dest = cmdutil.makefileobj(repo, dest)
        if not prefix:
            prefix = os.path.basename(repo.root) + "-%h"

    prefix = cmdutil.makefilename(repo, prefix, node)
    match = scmutil.match(ctx, [], opts)
    archival.archive(repo, dest, node, kind, not opts.get("no_decode"), match, prefix)


@command(
    "backout",
    [
        ("", "merge", None, _("merge with old dirstate parent after backout")),
        ("", "commit", None, _("commit if no conflicts were encountered (DEPRECATED)")),
        ("", "no-commit", None, _("do not commit")),
        (
            "",
            "parent",
            "",
            _("parent to choose when backing out merge (DEPRECATED)"),
            _("REV"),
        ),
        ("r", "rev", "", _("revision to backout"), _("REV")),
        ("e", "edit", False, _("invoke editor on commit messages")),
    ]
    + mergetoolopts
    + walkopts
    + commitopts
    + commitopts2,
    _("[OPTION]... [-r] REV"),
)
def backout(ui, repo, node=None, rev=None, **opts):
    """reverse effect of earlier changeset

    Prepare a new changeset with the effect of REV undone in the
    current working directory. If no conflicts were encountered,
    it will be committed immediately.

    If REV is the parent of the working directory, then this new changeset
    is committed automatically (unless --no-commit is specified).

    .. note::

       :hg:`backout` cannot be used to fix either an unwanted or
       incorrect merge.

    .. container:: verbose

      Examples:

      - Reverse the effect of the parent of the working directory.
        This backout will be committed immediately::

          hg backout -r .

      - Reverse the effect of previous bad revision 23::

          hg backout -r 23

      - Reverse the effect of previous bad revision 23 and
        leave changes uncommitted::

          hg backout -r 23 --no-commit
          hg commit -m "Backout revision 23"

      By default, the pending changeset will have one parent,
      maintaining a linear history. With --merge, the pending
      changeset will instead have two parents: the old parent of the
      working directory and a new child of REV that simply undoes REV.

      Before version 1.7, the behavior without --merge was equivalent
      to specifying --merge followed by :hg:`update --clean .` to
      cancel the merge and leave the child of REV as a head to be
      merged separately.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    See :hg:`help revert` for a way to restore files to the state
    of another revision.

    Returns 0 on success, 1 if nothing to backout or there are unresolved
    files.
    """
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        return _dobackout(ui, repo, node, rev, **opts)
    finally:
        release(lock, wlock)


def _dobackout(ui, repo, node=None, rev=None, **opts):
    opts = pycompat.byteskwargs(opts)
    if opts.get("commit") and opts.get("no_commit"):
        raise error.Abort(_("cannot use --commit with --no-commit"))
    if opts.get("merge") and opts.get("no_commit"):
        raise error.Abort(_("cannot use --merge with --no-commit"))

    if rev and node:
        raise error.Abort(_("please specify just one revision"))

    if not rev:
        rev = node

    if not rev:
        raise error.Abort(_("please specify a revision to backout"))

    date = opts.get("date")
    if date:
        opts["date"] = util.parsedate(date)

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)
    node = scmutil.revsingle(repo, rev).node()

    op1, op2 = repo.dirstate.parents()
    if not repo.changelog.isancestor(node, op1):
        raise error.Abort(_("cannot backout change that is not an ancestor"))

    p1, p2 = repo.changelog.parents(node)
    if p1 == nullid:
        raise error.Abort(_("cannot backout a change with no parents"))
    if p2 != nullid:
        if not opts.get("parent"):
            raise error.Abort(_("cannot backout a merge changeset"))
        p = repo.lookup(opts["parent"])
        if p not in (p1, p2):
            raise error.Abort(_("%s is not a parent of %s") % (short(p), short(node)))
        parent = p
    else:
        if opts.get("parent"):
            raise error.Abort(_("cannot use --parent on non-merge changeset"))
        parent = p1

    # the backout should appear on the same branch
    branch = repo.dirstate.branch()
    rctx = scmutil.revsingle(repo, hex(parent))
    if not opts.get("merge") and op1 != node:
        dsguard = dirstateguard.dirstateguard(repo, "backout")
        try:
            ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "backout")
            stats = mergemod.update(repo, parent, True, True, node, False)
            repo.setparents(op1, op2)
            dsguard.close()
            hg._showstats(repo, stats)
            if stats[3]:
                repo.ui.status(
                    _("use 'hg resolve' to retry unresolved " "file merges\n")
                )
                return 1
        finally:
            ui.setconfig("ui", "forcemerge", "", "")
            lockmod.release(dsguard)
    else:
        hg.clean(repo, node, show_stats=False)
        repo.dirstate.setbranch(branch)
        cmdutil.revert(ui, repo, rctx, repo.dirstate.parents())

    if opts.get("no_commit"):
        msg = _("changeset %s backed out, " "don't forget to commit.\n")
        ui.status(msg % short(node))
        return 0

    def commitfunc(ui, repo, message, match, opts):
        # take the first line of old description only, the title
        olddescription = repo.changelog.changelogrevision(
            node
        ).description.splitlines()[0]
        editform = "backout"
        e = cmdutil.getcommiteditor(editform=editform, **pycompat.strkwargs(opts))

        addmessage = "\n\nOriginal commit changeset: %s" % short(node)
        if not message:
            message = 'Back out "%s"' % olddescription
            e = cmdutil.getcommiteditor(edit=True, editform=editform)
        return repo.commit(
            message + addmessage, opts.get("user"), opts.get("date"), match, editor=e
        )

    newnode = cmdutil.commit(ui, repo, commitfunc, [], opts)
    if not newnode:
        ui.status(_("nothing changed\n"))
        return 1
    cmdutil.commitstatus(repo, newnode, branch)

    def nice(node):
        return "%d:%s" % (repo.changelog.rev(node), short(node))

    ui.status(
        _("changeset %s backs out changeset %s\n")
        % (nice(repo.changelog.tip()), nice(node))
    )
    if opts.get("merge") and op1 != node:
        hg.clean(repo, op1, show_stats=False)
        ui.status(_("merging with changeset %s\n") % nice(repo.changelog.tip()))
        try:
            ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "backout")
            return hg.merge(repo, hex(repo.changelog.tip()))
        finally:
            ui.setconfig("ui", "forcemerge", "", "")
    return 0


@command(
    "bisect",
    [
        ("r", "reset", False, _("reset bisect state")),
        ("g", "good", False, _("mark changeset good")),
        ("b", "bad", False, _("mark changeset bad")),
        ("s", "skip", False, _("skip testing changeset")),
        ("e", "extend", False, _("extend the bisect range")),
        ("c", "command", "", _("use command to check changeset state"), _("CMD")),
        ("U", "noupdate", False, _("do not update to target")),
        (
            "S",
            "nosparseskip",
            False,
            _("do not skip changesets with no changes in sparse profile"),
        ),
    ],
    _("[-gbsr] [-U] [-S] [-c CMD] [REV]"),
)
def bisect(
    ui,
    repo,
    rev=None,
    extra=None,
    command=None,
    reset=None,
    good=None,
    bad=None,
    skip=None,
    extend=None,
    noupdate=None,
    nosparseskip=None,
):
    """subdivision search of changesets

    This command helps to find changesets which introduce problems. To
    use, mark the earliest changeset you know exhibits the problem as
    bad, then mark the latest changeset which is free from the problem
    as good. Bisect will update your working directory to a revision
    for testing (unless the -U/--noupdate option is specified). Once
    you have performed tests, mark the working directory as good or
    bad, and bisect will either update to another candidate changeset
    or announce that it has found the bad revision. The command will
    also skip changesets, which don't contain changes in the sparse
    profile, unless the -S/--nosparseskip option is specified.

    As a shortcut, you can also use the revision argument to mark a
    revision as good or bad without checking it out first.

    If you supply a command, it will be used for automatic bisection.
    The environment variable HG_NODE will contain the ID of the
    changeset being tested. The exit status of the command will be
    used to mark revisions as good or bad: status 0 means good, 125
    means to skip the revision, 127 (command not found) will abort the
    bisection, and any other non-zero exit status means the revision
    is bad.

    .. container:: verbose

      Some examples:

      - start a bisection with known bad revision 34, and good revision 12::

          hg bisect --bad 34
          hg bisect --good 12

      - advance the current bisection by marking current revision as good or
        bad::

          hg bisect --good
          hg bisect --bad

      - mark the current revision, or a known revision, to be skipped (e.g. if
        that revision is not usable because of another issue)::

          hg bisect --skip
          hg bisect --skip 23

      - skip all revisions that do not touch directories ``foo`` or ``bar``::

          hg bisect --skip "!( file('path:foo') & file('path:bar') )"

      - forget the current bisection::

          hg bisect --reset

      - use 'make && make tests' to automatically find the first broken
        revision::

          hg bisect --reset
          hg bisect --bad 34
          hg bisect --good 12
          hg bisect --command "make && make tests"

      - see all changesets whose states are already known in the current
        bisection::

          hg log -r "bisect(pruned)"

      - see the changeset currently being bisected (especially useful
        if running with -U/--noupdate)::

          hg log -r "bisect(current)"

      - see all changesets that took part in the current bisection::

          hg log -r "bisect(range)"

      - you can even get a nice graph::

          hg log --graph -r "bisect(range)"

      See :hg:`help revisions.bisect` for more about the `bisect()` predicate.

    Returns 0 on success.
    """
    # backward compatibility
    if rev in "good bad reset init".split():
        ui.warn(_("(use of 'hg bisect <cmd>' is deprecated)\n"))
        cmd, rev, extra = rev, extra, None
        if cmd == "good":
            good = True
        elif cmd == "bad":
            bad = True
        else:
            reset = True
    elif extra:
        raise error.Abort(_("incompatible arguments"))

    incompatibles = {
        "--bad": bad,
        "--command": bool(command),
        "--extend": extend,
        "--good": good,
        "--reset": reset,
        "--skip": skip,
    }

    enabled = [x for x in incompatibles if incompatibles[x]]

    if len(enabled) > 1:
        raise error.Abort(_("%s and %s are incompatible") % tuple(sorted(enabled)[0:2]))

    if reset:
        hbisect.resetstate(repo)
        return

    state = hbisect.load_state(repo)

    # update state
    if good or bad or skip:
        if rev:
            nodes = [repo.lookup(i) for i in scmutil.revrange(repo, [rev])]
        else:
            nodes = [repo.lookup(".")]
        if good:
            state["good"] += nodes
        elif bad:
            state["bad"] += nodes
        elif skip:
            state["skip"] += nodes
        hbisect.save_state(repo, state)
        if not (state["good"] and state["bad"]):
            return

    def mayupdate(repo, node, show_stats=True):
        """common used update sequence"""
        if noupdate:
            return
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)
        return hg.clean(repo, node, show_stats=show_stats)

    def sparseskip(node, changesets, bgood, badnode, goodnode):
        """
        Skip testing next nodes if nothing was changed since last good or bad node
        in sparse profile.
        """

        nodestate = hbisect.checksparsebisectskip(repo, node, badnode, goodnode)
        while nodestate != "check":
            basenode = goodnode if nodestate == "good" else badnode
            skipsparsed = "Skipping changeset %d:%s as there are no changes inside\n\
the sparse profile from the known %s changeset %d:%s\n"
            ui.write(
                _(skipsparsed)
                % (
                    repo.changelog.rev(node),
                    short(node),
                    nodestate,
                    repo.changelog.rev(basenode),
                    short(basenode),
                )
            )

            state[nodestate].append(node)

            nodes, changesets, bgood, badnode, goodnode = hbisect.bisect(repo, state)
            node = nodes[0]

            # update state
            state["current"] = [node]
            hbisect.save_state(repo, state)

            if changesets == 0:
                return (node, changesets, bgood)
            nodestate = hbisect.checksparsebisectskip(repo, node, badnode, goodnode)

        return (node, changesets, bgood)

    displayer = cmdutil.show_changeset(ui, repo, {})

    if command:
        changesets = 1
        if noupdate:
            try:
                node = state["current"][0]
            except LookupError:
                raise error.Abort(
                    _(
                        "current bisect revision is unknown - "
                        "start a new bisect to fix"
                    )
                )
        else:
            node, p2 = repo.dirstate.parents()
            if p2 != nullid:
                raise error.Abort(_("current bisect revision is a merge"))
        if rev:
            node = repo[scmutil.revsingle(repo, rev, node)].node()
        try:
            while changesets:
                # update state
                state["current"] = [node]
                hbisect.save_state(repo, state)
                status = ui.system(
                    command, environ={"HG_NODE": hex(node)}, blockedtag="bisect_check"
                )
                if status == 125:
                    transition = "skip"
                elif status == 0:
                    transition = "good"
                # status < 0 means process was killed
                elif status == 127:
                    raise error.Abort(_("failed to execute %s") % command)
                elif status < 0:
                    raise error.Abort(_("%s killed") % command)
                else:
                    transition = "bad"
                state[transition].append(node)
                ctx = repo[node]
                ui.status(_("changeset %d:%s: %s\n") % (ctx, ctx, transition))
                hbisect.checkstate(state)
                # bisect
                nodes, changesets, bgood, badnode, goodnode = hbisect.bisect(
                    repo, state
                )
                node = nodes[0]

                if changesets != 0 and not nosparseskip:
                    node, changesets, bgood = sparseskip(
                        node, changesets, bgood, badnode, goodnode
                    )
                    if changesets == 0:
                        nodes = [node]
                        break

                mayupdate(repo, node, show_stats=False)
        finally:
            state["current"] = [node]
            hbisect.save_state(repo, state)
        hbisect.printresult(ui, repo, state, displayer, nodes, bgood)
        return

    hbisect.checkstate(state)

    # actually bisect
    nodes, changesets, good, badnode, goodnode = hbisect.bisect(repo, state)

    def showtestingnext(rev, node, changesets):
        # compute the approximate number of remaining tests
        tests, size = 0, 2
        while size <= changesets:
            tests, size = tests + 1, size * 2
        ui.write(
            _("Testing changeset %d:%s " "(%d changesets remaining, ~%d tests)\n")
            % (rev, short(node), changesets, tests)
        )

    # update to next check
    if extend:
        if not changesets:
            extendnode = hbisect.extendrange(repo, state, nodes, good)
            if extendnode is not None:
                extnode, extrev = extendnode.node(), extendnode.rev()
                state["current"] = [extnode]
                hbisect.save_state(repo, state)
                ui.write(
                    _("Extending search to changeset %d:%s\n") % (extrev, extendnode)
                )

                if not nosparseskip:
                    node, changesets, good = sparseskip(
                        extnode, changesets, good, badnode, goodnode
                    )
                    if node != extnode:
                        if changesets == 0:
                            hbisect.printresult(
                                ui, repo, state, displayer, [node], good
                            )
                            return
                        showtestingnext(repo.changelog.rev(node), node, changesets)
                        extnode = node

                return mayupdate(repo, extnode)
        raise error.Abort(_("nothing to extend"))

    if changesets == 0:
        hbisect.printresult(ui, repo, state, displayer, nodes, good)
    else:
        assert len(nodes) == 1  # only a single node can be tested next
        node = nodes[0]

        state["current"] = [node]
        hbisect.save_state(repo, state)

        if not nosparseskip:
            node, changesets, good = sparseskip(
                node, changesets, good, badnode, goodnode
            )
            if changesets == 0:
                hbisect.printresult(ui, repo, state, displayer, [node], good)
                return

        rev = repo.changelog.rev(node)
        showtestingnext(rev, node, changesets)

        return mayupdate(repo, node)


@command(
    "bookmarks|bookmark",
    [
        ("f", "force", False, _("force")),
        ("r", "rev", "", _("revision for bookmark action"), _("REV")),
        ("d", "delete", False, _("delete a given bookmark")),
        ("D", "strip", None, _("like --delete, but also strip changesets")),
        ("m", "rename", "", _("rename a given bookmark"), _("OLD")),
        ("i", "inactive", False, _("mark a bookmark inactive")),
    ]
    + formatteropts,
    _("hg bookmarks [OPTIONS]... [NAME]..."),
)
def bookmark(ui, repo, *names, **opts):
    """create a new bookmark or list existing bookmarks

    Bookmarks are labels on changesets to help track lines of development.
    Bookmarks are unversioned and can be moved, renamed and deleted.
    Deleting or moving a bookmark has no effect on the associated changesets.

    Creating or updating to a bookmark causes it to be marked as 'active'.
    The active bookmark is indicated with a '*'.
    When a commit is made, the active bookmark will advance to the new commit.
    A plain :hg:`update` will also advance an active bookmark, if possible.
    Updating away from a bookmark will cause it to be deactivated.

    Bookmarks can be pushed and pulled between repositories (see
    :hg:`help push` and :hg:`help pull`). If a shared bookmark has
    diverged, a new 'divergent bookmark' of the form 'name@path' will
    be created. Using :hg:`merge` will resolve the divergence.

    Specifying bookmark as '.' to -m or -d options is equivalent to specifying
    the active bookmark's name.

    A bookmark named '@' has the special property that :hg:`clone` will
    check it out by default if it exists.

    .. container:: verbose

      Examples:

      - create an active bookmark for a new line of development::

          hg book new-feature

      - create an inactive bookmark as a place marker::

          hg book -i reviewed

      - create an inactive bookmark on another changeset::

          hg book -r .^ tested

      - rename bookmark turkey to dinner::

          hg book -m turkey dinner

      - move the '@' bookmark from another branch::

          hg book -f @
    """
    force = opts.get(r"force")
    rev = opts.get(r"rev")
    delete = opts.get(r"delete")
    rename = opts.get(r"rename")
    inactive = opts.get(r"inactive")
    strip = opts.get("strip")

    if delete and rename:
        raise error.Abort(_("--delete and --rename are incompatible"))
    if delete and rev:
        raise error.Abort(_("--rev is incompatible with --delete"))
    if rename and rev:
        raise error.Abort(_("--rev is incompatible with --rename"))
    if not names and (delete or rev):
        raise error.Abort(_("bookmark name required"))
    if strip:
        # Check for incompatible options.
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
            # book --strip is just an alias for prune -B.
            # (it may raise UnknownCommand)
            stripfunc = cmdutil.findcmd("prune", table)[1][0]
            return stripfunc(ui, repo, bookmark=names, rev=[])

    if delete or rename or names or inactive:
        with repo.wlock(), repo.lock(), repo.transaction("bookmark") as tr:
            if delete:
                names = pycompat.maplist(repo._bookmarks.expandname, set(names))
                bookmarks.delete(repo, tr, names)
            elif rename:
                if not names:
                    raise error.Abort(_("new bookmark name required"))
                elif len(names) > 1:
                    raise error.Abort(_("only one new bookmark name allowed"))
                rename = repo._bookmarks.expandname(rename)
                bookmarks.rename(repo, tr, rename, names[0], force, inactive)
            elif names:
                bookmarks.addbookmarks(repo, tr, names, rev, force, inactive)
            elif inactive:
                if len(repo._bookmarks) == 0:
                    ui.status(_("no bookmarks set\n"))
                elif not repo._activebookmark:
                    ui.status(_("no active bookmark\n"))
                else:
                    bookmarks.deactivate(repo)
    else:  # show bookmarks
        bookmarks.printbookmarks(ui, repo, **opts)


@command(
    "branch",
    [
        ("f", "force", None, _("do nothing (DEPRECATED)")),
        ("C", "clean", None, _("raise an exception (DEPRECATED)")),
        # Note: for compatibility for tweakdefaults users
        ("", "new", None, _("do nothing (DEPRECATED)")),
    ],
    _("[-fC] [NAME]"),
)
def branch(ui, repo, label=None, **opts):
    """(deprecated. use 'hg bookmark' instead)

    This command does nothing meaningful and will be removed in the future.

    For now, it always prints "default" or raise an exception if NAME or -C is
    provided.
    """
    hintutil.trigger("branch-command-deprecate")
    if not opts.get("clean") and not label:
        ui.write("%s\n" % repo.dirstate.branch())
        return

    raise error.Abort(
        _("named branches are disabled in this repository"),
        hint=ui.config("ui", "disallowedbrancheshint", _("use bookmarks instead")),
    )


@command(
    "bundle",
    [
        ("f", "force", None, _("run even when the destination is unrelated")),
        (
            "r",
            "rev",
            [],
            _("a changeset intended to be added to the destination"),
            _("REV"),
        ),
        (
            "",
            "base",
            [],
            _("a base changeset assumed to be available at the destination"),
            _("REV"),
        ),
        ("a", "all", None, _("bundle all changesets in the repository")),
        ("t", "type", "bzip2", _("bundle compression type to use"), _("TYPE")),
    ]
    + remoteopts,
    _("[-f] [-t BUNDLESPEC] [-a] [-r REV]... [--base REV]... FILE [DEST]"),
)
def bundle(ui, repo, fname, dest=None, **opts):
    """create a bundle file

    Generate a bundle file containing data to be added to a repository.

    To create a bundle containing all changesets, use -a/--all
    (or --base null). Otherwise, hg assumes the destination will have
    all the nodes you specify with --base parameters. Otherwise, hg
    will assume the repository has all the nodes in destination, or
    default-push/default if no destination is specified.

    You can change bundle format with the -t/--type option. See
    :hg:`help bundlespec` for documentation on this format. By default,
    the most appropriate format is used and compression defaults to
    bzip2.

    The bundle file can then be transferred using conventional means
    and applied to another repository with the unbundle or pull
    command. This is useful when direct push and pull are not
    available or when exporting an entire repository is undesirable.

    Applying bundles preserves all changeset contents including
    permissions, copy/rename information, and revision history.

    Returns 0 on success, 1 if no changes found.
    """
    opts = pycompat.byteskwargs(opts)
    revs = None
    if "rev" in opts:
        revstrings = opts["rev"]
        revs = scmutil.revrange(repo, revstrings)
        if revstrings and not revs:
            raise error.Abort(_("no commits to bundle"))

    bundletype = opts.get("type", "bzip2").lower()
    try:
        bcompression, cgversion, params = exchange.parsebundlespec(
            repo, bundletype, strict=False
        )
    except error.UnsupportedBundleSpecification as e:
        raise error.Abort(
            str(e),
            hint=_("see 'hg help bundlespec' for supported " "values for --type"),
        )

    # Packed bundles are a pseudo bundle format for now.
    if cgversion == "s1":
        raise error.Abort(
            _('packed bundles cannot be produced by "hg bundle"'),
            hint=_("use 'hg debugcreatestreamclonebundle'"),
        )

    if opts.get("all"):
        if dest:
            raise error.Abort(
                _("--all is incompatible with specifying " "a destination")
            )
        if opts.get("base"):
            ui.warn(_("ignoring --base because --all was specified\n"))
        base = ["null"]
    else:
        base = scmutil.revrange(repo, opts.get("base"))
    if cgversion not in changegroup.supportedoutgoingversions(repo):
        raise error.Abort(
            _("repository does not support bundle version %s") % cgversion
        )

    if base:
        if dest:
            raise error.Abort(
                _("--base is incompatible with specifying " "a destination")
            )
        common = [repo.lookup(rev) for rev in base]
        heads = revs and map(repo.lookup, revs) or None
        outgoing = discovery.outgoing(repo, common, heads)
    else:
        dest = ui.expandpath(dest or "default-push", dest or "default")
        dest, branches = hg.parseurl(dest)
        other = hg.peer(repo, opts, dest)
        revs, checkout = hg.addbranchrevs(repo, repo, branches, revs)
        heads = revs and map(repo.lookup, revs) or revs
        outgoing = discovery.findcommonoutgoing(
            repo, other, onlyheads=heads, force=opts.get("force"), portable=True
        )

    if not outgoing.missing:
        scmutil.nochangesfound(ui, repo, not base and outgoing.excluded)
        return 1

    if cgversion == "01":  # bundle1
        if bcompression is None:
            bcompression = "UN"
        bversion = "HG10" + bcompression
        bcompression = None
    elif cgversion in ("02", "03"):
        bversion = "HG20"
    else:
        raise error.ProgrammingError(
            "bundle: unexpected changegroup version %s" % cgversion
        )

    # TODO compression options should be derived from bundlespec parsing.
    # This is a temporary hack to allow adjusting bundle compression
    # level without a) formalizing the bundlespec changes to declare it
    # b) introducing a command flag.
    compopts = {}
    complevel = ui.configint("experimental", "bundlecomplevel")
    if complevel is not None:
        compopts["level"] = complevel

    contentopts = {"cg.version": cgversion}
    if repo.ui.configbool("experimental", "evolution.bundle-obsmarker"):
        contentopts["obsolescence"] = True
    if repo.ui.configbool("experimental", "bundle-phases"):
        contentopts["phases"] = True
    bundle2.writenewbundle(
        ui,
        repo,
        "bundle",
        fname,
        bversion,
        outgoing,
        contentopts,
        compression=bcompression,
        compopts=compopts,
    )


@command(
    "cat",
    [
        ("o", "output", "", _("print output to file with formatted name"), _("FORMAT")),
        ("r", "rev", "", _("print the given revision"), _("REV")),
        ("", "decode", None, _("apply any matching decode filter")),
    ]
    + walkopts
    + formatteropts,
    _("[OPTION]... FILE..."),
    inferrepo=True,
    cmdtype=readonly,
)
def cat(ui, repo, file1, *pats, **opts):
    """output the current or given revision of files

    Print the specified files as they were at the given revision. If
    no revision is given, the parent of the working directory is used.

    Output may be to a file, in which case the name of the file is
    given using a format string. The formatting rules as follows:

    :``%%``: literal "%" character
    :``%s``: basename of file being printed
    :``%d``: dirname of file being printed, or '.' if in repository root
    :``%p``: root-relative path name of file being printed
    :``%H``: changeset hash (40 hexadecimal digits)
    :``%R``: changeset revision number
    :``%h``: short-form changeset hash (12 hexadecimal digits)
    :``%r``: zero-padded changeset revision number
    :``%b``: basename of the exporting repository

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    m = scmutil.match(ctx, (file1,) + pats, opts)
    fntemplate = opts.pop("output", "")
    if cmdutil.isstdiofilename(fntemplate):
        fntemplate = ""

    if fntemplate:
        fm = formatter.nullformatter(ui, "cat")
    else:
        ui.pager("cat")
        fm = ui.formatter("cat", opts)
    with fm:
        return cmdutil.cat(
            ui, repo, ctx, m, fm, fntemplate, "", **pycompat.strkwargs(opts)
        )


@command(
    "^clone",
    [
        (
            "U",
            "noupdate",
            None,
            _(
                "the clone will include an empty working "
                "directory (only a repository)"
            ),
        ),
        ("u", "updaterev", "", _("revision, tag, or branch to check out"), _("REV")),
        ("r", "rev", [], _("include the specified changeset"), _("REV")),
        ("", "pull", None, _("use pull protocol to copy metadata")),
        ("", "uncompressed", None, _("an alias to --stream (DEPRECATED)")),
        ("", "stream", None, _("clone with minimal data processing")),
    ]
    + remoteopts,
    _("[OPTION]... SOURCE [DEST]"),
    norepo=True,
)
def clone(ui, source, dest=None, **opts):
    """make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    ``.hg/hgrc`` file, as the default to be used for future pulls.

    Only local paths and ``ssh://`` URLs are supported as
    destinations. For ``ssh://`` destinations, no working directory or
    ``.hg/hgrc`` will be created on the remote side.

    If the source repository has a bookmark called '@' set, that
    revision will be checked out in the new repository by default.

    To check out a particular version, use -u/--update, or
    -U/--noupdate to create a clone with no working directory.

    To pull only a subset of changesets, specify one or more revisions
    identifiers with -r/--rev. The resulting clone will contain only the
    specified changesets and their ancestors. These options (or 'clone src#rev
    dest') imply --pull, even for local source repositories.

    In normal clone mode, the remote normalizes repository data into a common
    exchange format and the receiving end translates this data into its local
    storage format. --stream activates a different clone mode that essentially
    copies repository files from the remote with minimal data processing. This
    significantly reduces the CPU cost of a clone both remotely and locally.
    However, it often increases the transferred data size by 30-40%. This can
    result in substantially faster clones where I/O throughput is plentiful,
    especially for larger repositories. A side-effect of --stream clones is
    that storage settings and requirements on the remote are applied locally:
    a modern client may inherit legacy or inefficient storage used by the
    remote or a legacy Mercurial client may not be able to clone from a
    modern Mercurial remote.

    .. note::

       Specifying a tag will include the tagged changeset but not the
       changeset containing the tag.

    .. container:: verbose

      For efficiency, hardlinks are used for cloning whenever the
      source and destination are on the same filesystem (note this
      applies only to the repository data, not to the working
      directory). Some filesystems, such as AFS, implement hardlinking
      incorrectly, but do not report errors. In these cases, use the
      --pull option to avoid hardlinking.

      Mercurial will update the working directory to the first applicable
      revision from this list:

      a) null if -U or the source repository has no changesets
      b) if -u . and the source repository is local, the first parent of
         the source repository's working directory
      c) the changeset specified with -u (if a branch name, this means the
         latest head of that branch)
      d) the changeset specified with -r
      e) the tipmost head specified with -b
      f) the tipmost head specified with the url#branch source syntax
      g) the revision marked with the '@' bookmark, if present
      h) the tipmost head of the default branch
      i) tip

      When cloning from servers that support it, Mercurial may fetch
      pre-generated data from a server-advertised URL. When this is done,
      hooks operating on incoming changesets and changegroups may fire twice,
      once for the bundle fetched from the URL and another for any additional
      data not fetched from this URL. In addition, if an error occurs, the
      repository may be rolled back to a partial clone. This behavior may
      change in future releases. See :hg:`help -e clonebundles` for more.

      Examples:

      - clone a remote repository to a new directory named hg/::

          hg clone https://www.mercurial-scm.org/repo/hg/

      - create a lightweight local clone::

          hg clone project/ project-feature/

      - clone from an absolute path on an ssh server (note double-slash)::

          hg clone ssh://user@server//home/projects/alpha/

      - do a streaming clone while checking out a specified version::

          hg clone --stream http://server/repo -u 1.5

      - create a repository without changesets after a particular revision::

          hg clone -r 04e544 experimental/ good/

      - clone (and track) a particular named branch::

          hg clone https://www.mercurial-scm.org/repo/hg/#stable

    See :hg:`help urls` for details on specifying URLs.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    if opts.get("noupdate") and opts.get("updaterev"):
        raise error.Abort(_("cannot specify both --noupdate and --updaterev"))

    sharepool = ui.config("share", "pool")
    if sharepool:
        sharepool = util.expandpath(sharepool)
    shareopts = {"pool": sharepool, "mode": ui.config("share", "poolnaming")}

    r = hg.clone(
        ui,
        opts,
        source,
        dest,
        pull=opts.get("pull"),
        stream=opts.get("stream") or opts.get("uncompressed"),
        rev=opts.get("rev"),
        update=opts.get("updaterev") or not opts.get("noupdate"),
        shareopts=shareopts,
    )

    return r is None


@command(
    "^commit|ci",
    [
        (
            "A",
            "addremove",
            None,
            _("mark new/missing files as added/removed before committing"),
        ),
        ("", "amend", None, _("amend the parent of the working directory")),
        ("e", "edit", None, _("invoke editor on commit messages")),
        ("i", "interactive", None, _("use interactive mode")),
        ("M", "reuse-message", "", _("reuse commit message from REV"), _("REV")),
    ]
    + walkopts
    + commitopts
    + commitopts2,
    _("[OPTION]... [FILE]..."),
    inferrepo=True,
)
def commit(ui, repo, *pats, **opts):
    """save all pending changes or specified files in a new commit

    Commit changes to the given files to your local repository.

    By default, all pending changes (in other words, those reported by
    'hg status') are committed. If you want to commit only some of your
    changes, choose one of the following options:

    - Specify an exact list of files for which you want changes committed.

    - Use the -I or -X flags to pattern match file names to exclude or
      include by using a fileset. See 'hg help filesets' for more
      information.

    - Specify the --interactive flag to open a UI that will enable you
      to select individual insertions or deletions.

    If you are committing the result of a merge, such as when merge
    conflicts occur during 'hg checkout', commit all pending changes.
    Do not specify files or use -I, -X, or -i.

    Specify the -m flag to include a free-form commit message. If you do
    not specify -m, Mercurial opens your configured editor where you can
    enter a message based on a pre-loaded commit template.

    Returns 0 on success, 1 if nothing changed.

    .. container:: verbose

      If your commit fails, you can find a backup of your commit message in
      ``.hg/last-message.txt``.

      You can use --amend to replace your current commit with a new commit
      that contains the contents of the original commit, plus any pending
      changes. Specify -m to provide a new commit message. If you do not
      specify -m, Mercurial opens your configured editor where you can
      enter a message based on a pre-loaded commit template.

      .. note::

         'hg commit --amend' is not recommended. Use 'hg amend' instead.
         See 'hg help amend' for more information.

      Examples:

      - commit all files ending in .py::

          hg commit --include "set:**.py"

      - commit all non-binary files::

          hg commit --exclude "set:binary()"

      - amend the current commit and set the date to now::

          hg commit --amend --date now
    """
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        return _docommit(ui, repo, *pats, **opts)
    finally:
        release(lock, wlock)


def _docommit(ui, repo, *pats, **opts):
    if opts.get(r"interactive"):
        opts.pop(r"interactive")
        ret = cmdutil.dorecord(
            ui, repo, commit, None, False, cmdutil.recordfilter, *pats, **opts
        )
        # ret can be 0 (no changes to record) or the value returned by
        # commit(), 1 if nothing changed or None on success.
        return 1 if ret == 0 else ret

    opts = pycompat.byteskwargs(opts)

    # Allow the commit message from another commit to be reused.
    reuserev = opts.get("reuse_message")
    if reuserev:
        incompatibleopts = ["message", "logfile"]
        currentinvaliopts = [opt for opt in incompatibleopts if opts.get(opt)]
        if currentinvaliopts:
            raise error.Abort(
                _("--reuse-message and --%s are mutually exclusive")
                % (currentinvaliopts[0])
            )
        opts["message"] = scmutil.revsingle(repo, reuserev).description()

    cmdutil.checkunfinished(repo, commit=True)

    branch = repo[None].branch()

    extra = {}
    if opts.get("amend"):
        old = repo["."]
        rewriteutil.precheck(repo, [old.rev()], "amend")

        # Currently histedit gets confused if an amend happens while histedit
        # is in progress. Since we have a checkunfinished command, we are
        # temporarily honoring it.
        #
        # Note: eventually this guard will be removed. Please do not expect
        # this behavior to remain.
        if not obsolete.isenabled(repo, obsolete.createmarkersopt):
            cmdutil.checkunfinished(repo)

        node = cmdutil.amend(ui, repo, old, extra, pats, opts)
        if node == old.node():
            ui.status(_("nothing changed\n"))
            return 1
    else:
        commitextrafunc = opts.get("_commitextrafunc")
        if commitextrafunc is not None:
            commitextrafunc(extra)

        def commitfunc(ui, repo, message, match, opts):
            editform = cmdutil.mergeeditform(repo[None], "commit.normal")
            editor = cmdutil.getcommiteditor(
                editform=editform, **pycompat.strkwargs(opts)
            )
            return repo.commit(
                message,
                opts.get("user"),
                opts.get("date"),
                match,
                editor=editor,
                extra=extra,
            )

        node = cmdutil.commit(ui, repo, commitfunc, pats, opts)

        if not node:
            stat = cmdutil.postcommitstatus(repo, pats, opts)
            if stat[3]:
                ui.status(
                    _("nothing changed (%d missing files, see " "'hg status')\n")
                    % len(stat[3])
                )
            else:
                ui.status(_("nothing changed\n"))
            return 1

    cmdutil.commitstatus(repo, node, branch, opts=opts)


@command(
    "config|showconfig|debugconfig",
    [
        ("u", "untrusted", None, _("show untrusted configuration options")),
        ("e", "edit", None, _("edit user config")),
        ("l", "local", None, _("edit repository config")),
        ("g", "global", None, _("edit global config")),
    ]
    + formatteropts,
    _("[-u] [NAME]..."),
    optionalrepo=True,
    cmdtype=readonly,
)
def config(ui, repo, *values, **opts):
    """show config settings

    With no arguments, print names and values of all config items.

    With one argument of the form section.name, print just the value
    of that config item.

    With multiple arguments, print names and values of all config
    items with matching section names.

    With --edit, start an editor on the user-level config file. With
    --global, edit the system-wide config file. With --local, edit the
    repository-level config file.

    With --debug, the source (filename and line number) is printed
    for each config item.

    See :hg:`help config` for more information about config files.

    Returns 0 on success, 1 if NAME does not exist.

    """

    opts = pycompat.byteskwargs(opts)
    if opts.get("edit") or opts.get("local") or opts.get("global"):
        if opts.get("local") and opts.get("global"):
            raise error.Abort(_("can't use --local and --global together"))

        if opts.get("local"):
            if not repo:
                raise error.Abort(_("can't use --local outside a repository"))
            paths = [repo.localvfs.join("hgrc")]
        elif opts.get("global"):
            paths = rcutil.systemrcpath()
        else:
            paths = rcutil.userrcpath()

        for f in paths:
            if os.path.exists(f):
                break
        else:
            if opts.get("global"):
                samplehgrc = uimod.samplehgrcs["global"]
            elif opts.get("local"):
                samplehgrc = uimod.samplehgrcs["local"]
            else:
                samplehgrc = uimod.samplehgrcs["user"]

            f = paths[0]
            fp = open(f, "wb")
            fp.write(util.tonativeeol(samplehgrc))
            fp.close()

        editor = ui.geteditor()
        ui.system(
            '%s "%s"' % (editor, f),
            onerr=error.Abort,
            errprefix=_("edit failed"),
            blockedtag="config_edit",
        )
        return
    ui.pager("config")
    fm = ui.formatter("config", opts)
    untrusted = bool(opts.get("untrusted"))
    if values:
        sections = [v for v in values if "." not in v]
        items = [v for v in values if "." in v]
        if len(items) > 1 or items and sections:
            raise error.Abort(_("only one config item permitted"))
    matched = False
    for section, name, value in ui.walkconfig(untrusted=untrusted):
        source = ui.configsource(section, name, untrusted)
        value = pycompat.bytestr(value)
        if fm.isplain():
            source = source or "none"
            value = value.replace("\n", "\\n")
        entryname = section + "." + name
        if values:
            for v in values:
                if v == section:
                    fm.startitem()
                    fm.condwrite(ui.debugflag, "source", "%s: ", source)
                    fm.write("name value", "%s=%s\n", entryname, value)
                    matched = True
                elif v == entryname:
                    fm.startitem()
                    fm.condwrite(ui.debugflag, "source", "%s: ", source)
                    fm.write("value", "%s\n", value)
                    fm.data(name=entryname)
                    matched = True
        else:
            fm.startitem()
            fm.condwrite(ui.debugflag, "source", "%s: ", source)
            fm.write("name value", "%s=%s\n", entryname, value)
            matched = True
    fm.end()
    if matched:
        return 0
    return 1


@command(
    "copy|cp",
    [
        ("A", "after", None, _("record a copy that has already occurred")),
        ("f", "force", None, _("forcibly copy over an existing managed file")),
    ]
    + walkopts
    + dryrunopts,
    _("[OPTION]... [SOURCE]... DEST"),
)
def copy(ui, repo, *pats, **opts):
    """mark files as copied for the next commit

    Mark dest as having copies of source files. If dest is a
    directory, copies are put in that directory. If dest is a file,
    the source must be a single file.

    By default, this command copies the contents of files as they
    exist in the working directory. If invoked with -A/--after, the
    operation is recorded, but no copying is performed.

    This command takes effect with the next commit. To undo a copy
    before that, see :hg:`revert`.

    Returns 0 on success, 1 if errors are encountered.
    """
    opts = pycompat.byteskwargs(opts)
    with repo.wlock(False):
        return cmdutil.copy(ui, repo, pats, opts)


@command("debugcommands", [], _("[COMMAND]"), norepo=True)
def debugcommands(ui, cmd="", *args):
    """list all available commands and options"""
    for cmd, vals in sorted(table.iteritems()):
        cmd = cmd.split("|")[0].strip("^")
        opts = ", ".join([i[1] for i in vals[1]])
        ui.write("%s: %s\n" % (cmd, opts))


@command(
    "debugcomplete",
    [("o", "options", None, _("show the command options"))],
    _("[-o] CMD"),
    norepo=True,
)
def debugcomplete(ui, cmd="", **opts):
    """returns the completion list associated with the given command"""

    if opts.get(r"options"):
        options = []
        otables = [globalopts]
        if cmd:
            aliases, entry = cmdutil.findcmd(cmd, table, False)
            otables.append(entry[1])
        for t in otables:
            for o in t:
                if "(DEPRECATED)" in o[3]:
                    continue
                if o[0]:
                    options.append("-%s" % o[0])
                options.append("--%s" % o[1])
        ui.write("%s\n" % "\n".join(options))
        return

    cmdlist, unused_allcmds = cmdutil.findpossible(cmd, table)
    if ui.verbose:
        cmdlist = [" ".join(c[0]) for c in cmdlist.values()]
    ui.write("%s\n" % "\n".join(sorted(cmdlist)))


@command(
    "^diff",
    [
        ("r", "rev", [], _("revision"), _("REV")),
        ("c", "change", "", _("change made by revision"), _("REV")),
    ]
    + diffopts
    + diffopts2
    + walkopts,
    _("[OPTION]... ([-c REV] | [-r REV1 [-r REV2]]) [FILE]..."),
    inferrepo=True,
    cmdtype=readonly,
)
def diff(ui, repo, *pats, **opts):
    """show differences between commits

    Show the differences between two commits. If only one commit is specified,
    shows the differences between the specified commit and your pending
    changes. If no commits are specified, shows your pending changes.

    Specify -c to see the changes in the specified commit relative to its
    parent.

    By default, this command skips binary files. To override this behavior,
    specify -a to include binary files in the diff, probably with undesirable
    results.

    By default, diffs are shown using the unified diff format. Specify -g
    to generate diffs in the git extended diff format. For more information,
    read :hg:`help diffs`.

    .. note::

       :hg:`diff` might generate unexpected results during merges because it
       defaults to comparing against your checkout's first parent commit
       if no commits are specified.

    .. container:: verbose

      Examples:

      - compare a file in the current working directory to its parent::

          hg diff foo.c

      - compare two historical versions of a directory, with rename info::

          hg diff --git -r 1.0:1.2 lib/

      - get change stats relative to the last change on some date::

          hg diff --stat -r "date('may 2')"

      - diff all newly-added files that contain a keyword::

          hg diff "set:added() and grep(GNU)"

      - compare a revision and its parents::

          hg diff -c 9353         # compare against first parent
          hg diff -r 9353^:9353   # same using revset syntax
          hg diff -r 9353^2:9353  # compare against the second parent

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    revs = opts.get("rev")
    change = opts.get("change")
    stat = opts.get("stat")
    reverse = opts.get("reverse")
    onlyfilesinrevs = opts.get("only_files_in_revs")

    if revs and change:
        msg = _("cannot specify --rev and --change at the same time")
        raise error.Abort(msg)
    elif change:
        node2 = scmutil.revsingle(repo, change, None).node()
        node1 = repo[node2].p1().node()
    else:
        node1, node2 = scmutil.revpair(repo, revs)

    if reverse:
        node1, node2 = node2, node1

    if onlyfilesinrevs:
        files1 = set(repo[node1].files())
        files2 = set(repo[node2].files())
        pats = pats + tuple(repo.wvfs.join(f) for f in files1 | files2)

    diffopts = patch.diffallopts(ui, opts)
    m = scmutil.match(repo[node2], pats, opts)
    ui.pager("diff")
    cmdutil.diffordiffstat(
        ui, repo, diffopts, node1, node2, m, stat=stat, root=opts.get("root")
    )


@command(
    "^export",
    [
        ("o", "output", "", _("print output to file with formatted name"), _("FORMAT")),
        ("", "switch-parent", None, _("diff against the second parent")),
        ("r", "rev", [], _("revisions to export"), _("REV")),
    ]
    + diffopts,
    _("[OPTION]... [-o OUTFILESPEC] [-r] [REV]..."),
    cmdtype=readonly,
)
def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets

    Print the changeset header and diffs for one or more revisions.
    If no revision is given, the parent of the working directory is used.

    The information shown in the changeset header is: author, date,
    branch name (if non-default), changeset hash, parent(s) and commit
    comment.

    .. note::

       :hg:`export` may generate unexpected diff output for merge
       changesets, as it will compare the merge changeset against its
       first parent only.

    Output may be to a file, in which case the name of the file is
    given using a format string. The formatting rules are as follows:

    :``%%``: literal "%" character
    :``%H``: changeset hash (40 hexadecimal digits)
    :``%N``: number of patches being generated
    :``%R``: changeset revision number
    :``%b``: basename of the exporting repository
    :``%h``: short-form changeset hash (12 hexadecimal digits)
    :``%m``: first line of the commit message (only alphanumeric characters)
    :``%n``: zero-padded sequence number, starting at 1
    :``%r``: zero-padded changeset revision number

    Without the -a/--text option, export will avoid generating diffs
    of files it detects as binary. With -a, export will generate a
    diff anyway, probably with undesirable results.

    Use the -g/--git option to generate diffs in the git extended diff
    format. See :hg:`help diffs` for more information.

    With the --switch-parent option, the diff will be against the
    second parent. It can be useful to review a merge.

    .. container:: verbose

      Examples:

      - use export and import to transplant a bugfix to the current
        branch::

          hg export -r 9353 | hg import -

      - export all the changesets between two revisions to a file with
        rename information::

          hg export --git -r 123:150 > changes.txt

      - split outgoing changes into a series of patches with
        descriptive names::

          hg export -r "outgoing()" -o "%n-%m.patch"

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    changesets += tuple(opts.get("rev", []))
    if not changesets:
        changesets = ["."]
    revs = scmutil.revrange(repo, changesets)
    if not revs:
        raise error.Abort(_("export requires at least one changeset"))
    if len(revs) > 1:
        ui.note(_("exporting patches:\n"))
    else:
        ui.note(_("exporting patch:\n"))
    ui.pager("export")
    cmdutil.export(
        repo,
        revs,
        fntemplate=opts.get("output"),
        switch_parent=opts.get("switch_parent"),
        opts=patch.diffallopts(ui, opts),
    )


@command(
    "files",
    [
        ("r", "rev", "", _("search the repository as it is in REV"), _("REV")),
        ("0", "print0", None, _("end filenames with NUL, for use with xargs")),
    ]
    + walkopts
    + formatteropts,
    _("[OPTION]... [FILE]..."),
    cmdtype=readonly,
)
def files(ui, repo, *pats, **opts):
    """list tracked files

    Print files under Mercurial control in the working directory or
    specified revision for given files (excluding removed files).
    Files can be specified as filenames or filesets.

    If no files are given to match, this command prints the names
    of all files under Mercurial control.

    .. container:: verbose

      Examples:

      - list all files under the current directory::

          hg files .

      - shows sizes and flags for current revision::

          hg files -vr .

      - list all files named README::

          hg files -I "**/README"

      - list all binary files::

          hg files "set:binary()"

      - find files containing a regular expression::

          hg files "set:grep('bob')"

      - search tracked file contents with xargs and grep::

          hg files -0 | xargs -0 grep foo

    See :hg:`help patterns` and :hg:`help filesets` for more information
    on specifying file patterns.

    Returns 0 if a match is found, 1 otherwise.

    """

    opts = pycompat.byteskwargs(opts)
    ctx = scmutil.revsingle(repo, opts.get("rev"), None)

    end = "\n"
    if opts.get("print0"):
        end = "\0"
    fmt = "%s" + end

    m = scmutil.match(ctx, pats, opts)
    ui.pager("files")
    with ui.formatter("files", opts) as fm:
        return cmdutil.files(ui, ctx, m, fm, fmt)


@command("^forget", walkopts, _("[OPTION]... FILE..."), inferrepo=True)
def forget(ui, repo, *pats, **opts):
    """stop tracking the specified files

    Mark the specified files so they will no longer be tracked
    after the next commit.

    This only removes files from the current branch, not from the
    entire project history, and it does not delete them from the
    working directory.

    To delete the file from the working directory, see :hg:`remove`.

    To undo a forget before the next commit, see :hg:`add`.

    .. container:: verbose

      Examples:

      - forget newly-added binary files::

          hg forget "set:added() and binary()"

      - forget files that would be excluded by .gitignore::

          hg forget "set:gitignore()"

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    if not pats:
        raise error.Abort(_("no files specified"))

    m = scmutil.match(repo[None], pats, opts)
    rejected = cmdutil.forget(ui, repo, m, prefix="", explicitonly=False)[0]
    return rejected and 1 or 0


@command(
    "graft",
    [
        ("r", "rev", [], _("revisions to graft"), _("REV")),
        ("c", "continue", False, _("resume interrupted graft")),
        ("", "abort", False, _("abort an interrupted graft")),
        ("e", "edit", False, _("invoke editor on commit messages")),
        ("", "log", None, _("append graft info to log message")),
        ("f", "force", False, _("force graft")),
        ("D", "currentdate", False, _("record the current date as commit date")),
        (
            "U",
            "currentuser",
            False,
            _("record the current user as committer"),
            _("DATE"),
        ),
    ]
    + commitopts2
    + mergetoolopts
    + dryrunopts,
    _("[OPTION]... [-r REV]... REV..."),
)
def graft(ui, repo, *revs, **opts):
    """copy commits from a different location

    This command uses Mercurial's merge logic to copy individual
    changes from other branches without merging branches in the
    history graph. This is sometimes known as 'backporting' or
    'cherry-picking'. By default, graft will copy user, date, and
    description from the source changesets.

    Changesets that are ancestors of the current revision, that have
    already been grafted, or that are merges will be skipped.

    If --log is specified, log messages will have a comment appended
    of the form::

      (grafted from CHANGESETHASH)

    If --force is specified, revisions will be grafted even if they
    are already ancestors of, or have been grafted to, the destination.
    This is useful when the revisions have since been backed out.

    If a graft merge results in conflicts, the graft process is
    interrupted so that the current merge can be manually resolved.
    Once all conflicts are addressed, the graft process can be
    continued with the -c/--continue option.

    .. note::

       The -c/--continue option does not reapply earlier options, except
       for --force.

    .. container:: verbose

      Examples:

      - copy a single change to the stable branch and edit its description::

          hg update stable
          hg graft --edit 9393

      - graft a range of changesets with one exception, updating dates::

          hg graft -D "2085::2093 and not 2091"

      - continue a graft after resolving conflicts::

          hg graft -c

      - abort an interrupted graft::

          hg graft --abort

      - show the source of a grafted changeset::

          hg log --debug -r .

      - show revisions sorted by date::

          hg log -r "sort(all(), date)"

    See :hg:`help revisions` for more about specifying revisions.

    Returns 0 on successful completion.
    """
    with repo.wlock():
        return _dograft(ui, repo, *revs, **opts)


def _dograft(ui, repo, *revs, **opts):
    opts = pycompat.byteskwargs(opts)
    if revs and opts.get("rev"):
        ui.warn(
            _(
                "warning: inconsistent use of --rev might give unexpected "
                "revision ordering!\n"
            )
        )

    revs = list(revs)
    revs.extend(opts.get("rev"))

    if not opts.get("user") and opts.get("currentuser"):
        opts["user"] = ui.username()
    if not opts.get("date") and opts.get("currentdate"):
        opts["date"] = "%d %d" % util.makedate()

    editor = cmdutil.getcommiteditor(editform="graft", **pycompat.strkwargs(opts))

    cont = False
    if opts.get("continue") or opts.get("abort"):
        if revs and opts.get("continue"):
            raise error.Abort(_("can't specify --continue and revisions"))
        if revs and opts.get("abort"):
            raise error.Abort(_("can't specify --abort and revisions"))

        # read in unfinished revisions
        try:
            nodes = repo.localvfs.read("graftstate").splitlines()
            revs = [repo[node].rev() for node in nodes]
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            cmdutil.wrongtooltocontinue(repo, _("graft"))

        if opts.get("continue"):
            cont = True
        if opts.get("abort"):
            return update(ui, repo, node=".", clean=True)
    else:
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)
        if not revs:
            raise error.Abort(_("no revisions specified"))
        revs = scmutil.revrange(repo, revs)

    skipped = set()
    # check for merges
    for rev in repo.revs("%ld and merge()", revs):
        ui.warn(_("skipping ungraftable merge revision %d\n") % rev)
        skipped.add(rev)
    revs = [r for r in revs if r not in skipped]
    if not revs:
        raise error.Abort(_("empty revision set was specified"))

    # Don't check in the --continue case, in effect retaining --force across
    # --continues. That's because without --force, any revisions we decided to
    # skip would have been filtered out here, so they wouldn't have made their
    # way to the graftstate. With --force, any revisions we would have otherwise
    # skipped would not have been filtered out, and if they hadn't been applied
    # already, they'd have been in the graftstate.
    if not (cont or opts.get("force")):
        # check for ancestors of dest branch
        crev = repo["."].rev()
        ancestors = repo.changelog.ancestors([crev], inclusive=True)
        # XXX make this lazy in the future
        # don't mutate while iterating, create a copy
        for rev in list(revs):
            if rev in ancestors:
                ui.warn(_("skipping ancestor revision %d:%s\n") % (rev, repo[rev]))
                # XXX remove on list is slow
                revs.remove(rev)
        if not revs:
            return -1

        # analyze revs for earlier grafts
        ids = {}
        for ctx in repo.set("%ld", revs):
            ids[ctx.hex()] = ctx.rev()
            n = ctx.extra().get("source")
            if n:
                ids[n] = ctx.rev()

        # check ancestors for earlier grafts
        ui.debug("scanning for duplicate grafts\n")

        # The only changesets we can be sure doesn't contain grafts of any
        # revs, are the ones that are common ancestors of *all* revs:
        for rev in repo.revs("only(%d,ancestor(%ld))", crev, revs):
            ctx = repo[rev]
            n = ctx.extra().get("source")
            if n in ids:
                try:
                    r = repo[n].rev()
                except error.RepoLookupError:
                    r = None
                if r in revs:
                    ui.warn(
                        _("skipping revision %d:%s " "(already grafted to %d:%s)\n")
                        % (r, repo[r], rev, ctx)
                    )
                    revs.remove(r)
                elif ids[n] in revs:
                    if r is None:
                        ui.warn(
                            _(
                                "skipping already grafted revision %d:%s "
                                "(%d:%s also has unknown origin %s)\n"
                            )
                            % (ids[n], repo[ids[n]], rev, ctx, n[:12])
                        )
                    else:
                        ui.warn(
                            _(
                                "skipping already grafted revision %d:%s "
                                "(%d:%s also has origin %d:%s)\n"
                            )
                            % (ids[n], repo[ids[n]], rev, ctx, r, n[:12])
                        )
                    revs.remove(ids[n])
            elif ctx.hex() in ids:
                r = ids[ctx.hex()]
                ui.warn(
                    _(
                        "skipping already grafted revision %d:%s "
                        "(was grafted from %d:%s)\n"
                    )
                    % (r, repo[r], rev, ctx)
                )
                revs.remove(r)
        if not revs:
            return -1

    for pos, ctx in enumerate(repo.set("%ld", revs)):
        desc = '%d:%s "%s"' % (ctx.rev(), ctx, ctx.description().split("\n", 1)[0])
        names = repo.nodetags(ctx.node()) + repo.nodebookmarks(ctx.node())
        if names:
            desc += " (%s)" % " ".join(names)
        ui.status(_("grafting %s\n") % desc)
        if opts.get("dry_run"):
            continue

        source = ctx.extra().get("source")
        extra = {}
        if source:
            extra["source"] = source
            extra["intermediate-source"] = ctx.hex()
        else:
            extra["source"] = ctx.hex()
        user = ctx.user()
        if opts.get("user"):
            user = opts["user"]
        date = ctx.date()
        if opts.get("date"):
            date = opts["date"]
        message = ctx.description()
        if opts.get("log"):
            message += "\n(grafted from %s)" % ctx.hex()

        # we don't merge the first commit when continuing
        if not cont:
            # perform the graft merge with p1(rev) as 'ancestor'
            try:
                # ui.forcemerge is an internal variable, do not document
                repo.ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "graft")
                stats = mergemod.graft(repo, ctx, ctx.p1(), ["local", "graft"])
            finally:
                repo.ui.setconfig("ui", "forcemerge", "", "graft")
            # report any conflicts
            if stats and stats[3] > 0:
                # write out state for --continue
                nodelines = [repo[rev].hex() + "\n" for rev in revs[pos:]]
                repo.localvfs.write("graftstate", "".join(nodelines))
                extra = ""
                if opts.get("user"):
                    extra += " --user %s" % util.shellquote(opts["user"])
                if opts.get("date"):
                    extra += " --date %s" % util.shellquote(opts["date"])
                if opts.get("log"):
                    extra += " --log"
                hint = _("use 'hg resolve' and 'hg graft --continue%s'") % extra
                raise error.Abort(_("unresolved conflicts, can't continue"), hint=hint)
        else:
            cont = False

        # commit
        node = repo.commit(
            text=message, user=user, date=date, extra=extra, editor=editor
        )
        if node is None:
            ui.warn(
                _("note: graft of %d:%s created no changes to commit\n")
                % (ctx.rev(), ctx)
            )

    # remove state when we complete successfully
    if not opts.get("dry_run"):
        repo.localvfs.unlinkpath("graftstate", ignoremissing=True)

    return 0


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

    For the old 'hg grep', which searches through history, see 'histgrep'."""
    # XXX: The current implementation heavily depends on external programs like
    # grep, xargs and biggrep.  Command-line flag support is a bit messy.  It
    # does not look like a source control command (ex. no --rev support).
    # Ideally, `grep` is just `histgrep -r 'wdir()'` and they can be merged.
    # Possible future work are:
    # - For "searching many files in a single revision" use-case, utilize
    #   ripgrep's logic. That "single revision" includes "wdir()". Get rid
    #   of shelling out to grep or xargs.
    # - For "searching few files in a range of revisions" use-case, maybe
    #   try using fastannoate logic.
    # - Find a cleaner way to integrate with FB-only biggrep and fallback
    #   gracefully.
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

    biggrepclient = ui.config(
        "grep",
        "biggrepclient",
        "/usr/local/fbprojects/packages/biggrep.client/stable/biggrep_client",
    )
    biggreptier = ui.config("grep", "biggreptier", "biggrep.master")
    biggrepcorpus = ui.config("grep", "biggrepcorpus")

    # If true, we'll use biggrepclient to perform the grep against some
    # externally maintained index.  We don't provide an implementation
    # of that tool with this repo, just the optional client interface.
    biggrep = ui.configbool("grep", "usebiggrep", None)

    if biggrep is None:
        if (
            "eden" in repo.requirements
            and biggrepcorpus
            and os.path.exists(biggrepclient)
        ):
            biggrep = True

    # Ask big grep to strip out the corpus dir (stripdir) and to include
    # the corpus revision on the first line.
    biggrepcmd = [
        biggrepclient,
        "--stripdir",
        "-r",
        "--expression",
        pattern,
        biggreptier,
        biggrepcorpus,
        "re2",
    ]

    args = []

    if opts.get("after_context"):
        args.append("-A")
        args.append(opts.get("after_context"))
    if opts.get("before_context"):
        args.append("-B")
        args.append(opts.get("before_context"))
    if opts.get("context"):
        args.append("-C")
        args.append(opts.get("context"))
    if opts.get("ignore_case"):
        args.append("-i")
    if opts.get("files_with_matches"):
        args.append("-l")
    if opts.get("line_number"):
        cmd.append("-n")
    if opts.get("invert_match"):
        if biggrep:
            raise error.Abort("Cannot use invert_match option with big grep")
        cmd.append("-v")
    if opts.get("word_regexp"):
        cmd.append("-w")
        biggrepcmd[4] = "\\b%s\\b" % pattern
    if opts.get("extended_regexp"):
        cmd.append("-E")
        # re2 is already mostly compatible by default, so there are no options
        # to apply for this.
    if opts.get("fixed_strings"):
        cmd.append("-F")
        # using bgs rather than bgr switches the engine to fixed string matches
        biggrepcmd[0] = "bgs"
    if opts.get("perl_regexp"):
        cmd.append("-P")
        # re2 is already mostly pcre compatible, so there are no options
        # to apply for this.

    biggrepcmd += args
    cmd += args

    # color support, using the color extension
    colormode = getattr(ui, "_colormode", "")
    if colormode == "ansi":
        cmd.append("--color=always")
        biggrepcmd.append("--color=on")

    # Copy match specific options
    match_opts = {}
    for k in ("include", "exclude"):
        if k in opts:
            match_opts[k] = opts.get(k)

    reporoot = os.path.dirname(repo.path)
    wctx = repo[None]
    if not pats:
        # Search everything in the current directory
        m = scmutil.match(wctx, ["."], match_opts)
        # Scope biggrep to the cwd equivalent path, relative to the root
        # of its corpus.
        biggrepcmd += ["-f", util.pathto(repo.getcwd(), reporoot, ".")]
    else:
        # Search using the specified patterns
        m = scmutil.match(wctx, pats, match_opts)
        # Scope biggrep to the same set of patterns.  Ideally we'd have
        # a way to translate the matcher object to a regex, but we don't
        # so we cross fingers and hope that the patterns are simple filenames.
        biggrepcmd += [
            "-f",
            "(%s)" % "|".join([util.pathto(repo.getcwd(), reporoot, f) for f in pats]),
        ]

    # Add '--' to make sure grep recognizes all remaining arguments
    # (passed in by xargs) as filenames.
    cmd.append("--")
    ui.pager("grep")

    if biggrep:
        p = subprocess.Popen(
            biggrepcmd,
            bufsize=-1,
            close_fds=util.closefds,
            stdout=subprocess.PIPE,
            cwd=reporoot,
        )
        out, err = p.communicate()
        lines = out.rstrip().split("\n")
        # the first line has the revision for the corpus; parse it out
        # the format is "#HASH:timestamp"
        revisionline = lines[0][1:]
        corpusrev, timestamp = revisionline.split(":", 1)
        lines = lines[1:]

        resultsbyfile = {}
        includelineno = opts.get("line_number")
        fileswithmatches = opts.get("files_with_matches")

        for line in lines:
            try:
                filename, lineno, colno, context = line.split(":", 3)
            except Exception:
                binaryfile = re.match("Binary file (.*) matches", line)
                if binaryfile:
                    filename = binaryfile.group(1)
                    lineno = 0
                    colno = 0
                    context = None
                elif fileswithmatches:
                    filename = line
                else:
                    # If we couldn't parse the line, just pass it thru
                    ui.write(line)
                    ui.write("\n")
                    continue

            unescapedfilename = util.stripansiescapes(filename)

            # filter to just the files that match the list supplied
            # by the caller
            if m(unescapedfilename):
                # relativize the path to the CWD.  Note that `filename` will
                # often have escape sequences, so we do a substring replacement
                filename = filename.replace(unescapedfilename, m.rel(unescapedfilename))

                if unescapedfilename not in resultsbyfile:
                    resultsbyfile[unescapedfilename] = []

                # re-assemble the output
                if fileswithmatches:
                    resultsbyfile[unescapedfilename].append("%s\n" % filename)
                elif lineno == 0 and colno == 0 and context is None:
                    # Take care with binary file matches!
                    resultsbyfile[unescapedfilename].append(
                        "Binary file %s matches\n" % filename
                    )
                elif includelineno:
                    resultsbyfile[unescapedfilename].append(
                        "%s:%s:%s\n" % (filename, lineno, context)
                    )
                else:
                    resultsbyfile[unescapedfilename].append(
                        "%s:%s\n" % (filename, context)
                    )

        # Now check to see what has changed since the corpusrev
        # we're going to need to grep those and stitch the results together
        try:
            needbin = True
            svnmeta = getattr(repo, "svnmeta", None)
            if svnmeta:
                meta = svnmeta(skiperrorcheck=True)
                if meta.revmapexists:
                    corpusbin = meta.revmap.get((int(corpusrev), None))
                    # If None is returned, we don't have information about
                    # that revision locally, so trigger the pull instructions.
                    if corpusbin is None:
                        raise error.RepoLookupError
                    needbin = False

            if needbin:
                corpusbin = bin(corpusrev)

            changes = repo.status(corpusbin, None, m)
        except error.RepoLookupError:
            # TODO: can we trigger a commit cloud fetch for this case?

            # print the results we've gathered so far.  We're not sure
            # how things differ, so we'll follow up with a warning.
            for lines in resultsbyfile.values():
                for line in lines:
                    ui.write(line)

            ui.warn(
                _(
                    "The results above are based on revision %s\n"
                    "which is not available locally and thus may be inaccurate.\n"
                    "To get accurate results, run `hg pull` and re-run "
                    "your grep.\n"
                )
                % corpusrev
            )
            return

        # which files we're going to search locally
        filestogrep = set()

        # files that have been changed or added need to be searched again
        for f in changes.modified:
            resultsbyfile.pop(f, None)
            filestogrep.add(f)
        for f in changes.added:
            resultsbyfile.pop(f, None)
            filestogrep.add(f)

        # files that have been removed since the corpus rev cannot match
        for f in changes.removed:
            resultsbyfile.pop(f, None)
        for f in changes.deleted:
            resultsbyfile.pop(f, None)

        # Having filtered out the changed files from the big grep results,
        # we can now print those that remain.
        for lines in resultsbyfile.values():
            for line in lines:
                ui.write(line)

        # pass on any changed files to the local grep
        if len(filestogrep) > 0:
            # Ensure that the biggrep results are flushed before we
            # start to intermingle with the local grep process output
            ui.flush()
            return _rungrep(cmd, sorted(filestogrep), m)

        return 0

    ds = repo.dirstate
    getkind = stat.S_IFMT
    lnkkind = stat.S_IFLNK
    results = ds.walk(m, unknown=False, ignored=False)

    files = []
    for f in sorted(results.keys()):
        st = results[f]
        # skip symlinks and removed files
        if st is None or getkind(st.st_mode) == lnkkind:
            continue
        files.append(f)

    return _rungrep(cmd, files, m)


def _rungrep(cmd, files, match):
    p = subprocess.Popen(
        cmd, bufsize=-1, close_fds=util.closefds, stdin=subprocess.PIPE
    )
    write = p.stdin.write
    for f in files:
        write(match.rel(f) + "\0")

    p.stdin.close()
    return p.wait()


@command(
    "heads",
    [
        (
            "r",
            "rev",
            "",
            _("show only heads which are descendants of STARTREV"),
            _("STARTREV"),
        ),
        ("t", "topo", False, _("show topological heads only")),
        ("a", "active", False, _("show active branchheads only (DEPRECATED)")),
        ("c", "closed", False, _("show normal and closed branch heads")),
    ]
    + templateopts,
    _("[-ct] [-r STARTREV] [REV]..."),
    cmdtype=readonly,
)
def heads(ui, repo, *branchrevs, **opts):
    """show branch heads

    With no arguments, show all open branch heads in the repository.
    Branch heads are changesets that have no descendants on the
    same branch. They are where development generally takes place and
    are the usual targets for update and merge operations.

    If one or more REVs are given, only open branch heads on the
    branches associated with the specified changesets are shown. This
    means that you can use :hg:`heads .` to see the heads on the
    currently checked-out branch.

    If -c/--closed is specified, also show branch heads marked closed
    (see :hg:`commit --close-branch`).

    If STARTREV is specified, only those heads that are descendants of
    STARTREV will be displayed.

    If -t/--topo is specified, named branch mechanics will be ignored and only
    topological heads (changesets with no children) will be shown.

    Returns 0 if matching heads are found, 1 if not.
    """

    opts = pycompat.byteskwargs(opts)
    start = None
    if "rev" in opts:
        start = scmutil.revsingle(repo, opts["rev"], None).node()

    if opts.get("topo"):
        heads = [repo[h] for h in repo.heads(start)]
    else:
        heads = []
        for branch in repo.branchmap():
            heads += repo.branchheads(branch, start, opts.get("closed"))
        heads = [repo[h] for h in heads]

    if branchrevs:
        branches = set(repo[br].branch() for br in branchrevs)
        heads = [h for h in heads if h.branch() in branches]

    if opts.get("active") and branchrevs:
        dagheads = repo.heads(start)
        heads = [h for h in heads if h.node() in dagheads]

    if branchrevs:
        haveheads = set(h.branch() for h in heads)
        if branches - haveheads:
            headless = ", ".join(b for b in branches - haveheads)
            msg = _("no open branch heads found on branches %s")
            if opts.get("rev"):
                msg += _(" (started at %s)") % opts["rev"]
            ui.warn((msg + "\n") % headless)

    if not heads:
        return 1

    ui.pager("heads")
    heads = sorted(heads, key=lambda x: -x.rev())
    displayer = cmdutil.show_changeset(ui, repo, opts)
    for ctx in heads:
        displayer.show(ctx)
    displayer.close()


@command(
    "help",
    [
        ("e", "extension", None, _("show help for extensions")),
        ("c", "command", None, _("show help for commands")),
        ("k", "keyword", None, _("show topics matching keyword")),
        ("s", "system", [], _("show help for specific platform(s)")),
    ],
    _("[-ecks] [TOPIC]"),
    norepo=True,
    cmdtype=readonly,
)
def help_(ui, *names, **opts):
    """show help for a given topic or a help overview

    With no arguments, print a list of commands with short help messages.

    Given a topic, extension, or command name, print help for that
    topic.

    Returns 0 if successful.
    """

    name = " ".join(names) if names and names != (None,) else None
    keep = opts.get(r"system") or []
    if len(keep) == 0:
        if pycompat.sysplatform.startswith("win"):
            keep.append("windows")
        elif pycompat.sysplatform == "OpenVMS":
            keep.append("vms")
        elif pycompat.sysplatform == "plan9":
            keep.append("plan9")
        else:
            keep.append("unix")
            keep.append(pycompat.sysplatform.lower())
    if ui.verbose:
        keep.append("verbose")

    commands = sys.modules[__name__]
    formatted = help.formattedhelp(ui, commands, name, keep=keep, **opts)
    ui.pager("help")
    ui.write(formatted)


@command(
    "hint",
    [("", "ack", False, _("acknowledge and silence hints"))],
    _("[--ack] NAME ..."),
    norepo=True,
)
def hint(ui, *names, **opts):
    """acknowledge hints

    ``hg hint --ack NAME`` modifies hgrc to silence hints starting with
    ``hint[NAME]``.
    """
    if opts.get("ack") and names:
        hintutil.silence(ui, names)
        if not ui.quiet:
            ui.write(_("hints about %s are silenced\n") % _(", ").join(names))


@command(
    "histgrep",
    [
        ("0", "print0", None, _("end fields with NUL")),
        ("", "all", None, _("print all revisions that match")),
        ("a", "text", None, _("treat all files as text")),
        (
            "f",
            "follow",
            None,
            _("follow changeset history," " or file history across copies and renames"),
        ),
        ("i", "ignore-case", None, _("ignore case when matching")),
        (
            "l",
            "files-with-matches",
            None,
            _("print only filenames and revisions that match"),
        ),
        ("n", "line-number", None, _("print matching line numbers")),
        (
            "r",
            "rev",
            [],
            _("only search files changed within revision range"),
            _("REV"),
        ),
        ("u", "user", None, _("list the author (long with -v)")),
        ("d", "date", None, _("list the date (short with -q)")),
    ]
    + formatteropts
    + walkopts,
    _("[OPTION]... PATTERN [FILE]..."),
    inferrepo=True,
    cmdtype=readonly,
)
def histgrep(ui, repo, pattern, *pats, **opts):
    """search backwards through history for a pattern in the specified files

    Search revision history for a regular expression in the specified
    files or the entire project.

    By default, grep prints the most recent revision number for each
    file in which it finds a match. To get it to print every revision
    that contains a change in match status ("-" for a match that becomes
    a non-match, or "+" for a non-match that becomes a match), use the
    --all flag.

    PATTERN can be any Python (roughly Perl-compatible) regular
    expression.

    If no FILEs are specified (and -f/--follow isn't set), all files in
    the repository are searched, including those that don't exist in the
    current branch or have been deleted in a prior changeset.

    .. container:: verbose

      ``histgrep.allowfullrepogrep`` controls whether the entire repo can be
      queried without any patterns, which can be expensive in big repositories.

    Returns 0 if a match is found, 1 otherwise.
    """
    if not pats and not ui.configbool("histgrep", "allowfullrepogrep"):
        m = _("can't run histgrep on the whole repo, please provide filenames")
        h = _("this is disabled to avoid very slow greps over the whole repo")
        raise error.Abort(m, hint=h)

    opts = pycompat.byteskwargs(opts)
    reflags = re.M
    if opts.get("ignore_case"):
        reflags |= re.I
    try:
        regexp = util.re.compile(pattern, reflags)
    except re.error as inst:
        ui.warn(_("grep: invalid match pattern: %s\n") % inst)
        return 1
    sep, eol = ":", "\n"
    if opts.get("print0"):
        sep = eol = "\0"

    getfile = util.lrucachefunc(repo.file)

    def matchlines(body):
        begin = 0
        linenum = 0
        while begin < len(body):
            match = regexp.search(body, begin)
            if not match:
                break
            mstart, mend = match.span()
            linenum += body.count("\n", begin, mstart) + 1
            lstart = body.rfind("\n", begin, mstart) + 1 or begin
            begin = body.find("\n", mend) + 1 or len(body) + 1
            lend = begin - 1
            yield linenum, mstart - lstart, mend - lstart, body[lstart:lend]

    class linestate(object):
        def __init__(self, line, linenum, colstart, colend):
            self.line = line
            self.linenum = linenum
            self.colstart = colstart
            self.colend = colend

        def __hash__(self):
            return hash((self.linenum, self.line))

        def __eq__(self, other):
            return self.line == other.line

        def findpos(self):
            """Iterate all (start, end) indices of matches"""
            yield self.colstart, self.colend
            p = self.colend
            while p < len(self.line):
                m = regexp.search(self.line, p)
                if not m:
                    break
                yield m.span()
                p = m.end()

    matches = {}
    copies = {}

    def grepbody(fn, rev, body):
        matches[rev].setdefault(fn, [])
        m = matches[rev][fn]
        for lnum, cstart, cend, line in matchlines(body):
            s = linestate(line, lnum, cstart, cend)
            m.append(s)

    def difflinestates(a, b):
        sm = difflib.SequenceMatcher(None, a, b)
        for tag, alo, ahi, blo, bhi in sm.get_opcodes():
            if tag == "insert":
                for i in xrange(blo, bhi):
                    yield ("+", b[i])
            elif tag == "delete":
                for i in xrange(alo, ahi):
                    yield ("-", a[i])
            elif tag == "replace":
                for i in xrange(alo, ahi):
                    yield ("-", a[i])
                for i in xrange(blo, bhi):
                    yield ("+", b[i])

    def display(fm, fn, ctx, pstates, states):
        rev = ctx.rev()
        if fm.isplain():
            formatuser = ui.shortuser
        else:
            formatuser = str
        if ui.quiet:
            datefmt = "%Y-%m-%d"
        else:
            datefmt = "%a %b %d %H:%M:%S %Y %1%2"
        found = False

        @util.cachefunc
        def binary():
            flog = getfile(fn)
            return util.binary(flog.read(ctx.filenode(fn)))

        fieldnamemap = {"filename": "file", "linenumber": "line_number"}
        if opts.get("all"):
            iter = difflinestates(pstates, states)
        else:
            iter = [("", l) for l in states]
        for change, l in iter:
            fm.startitem()
            fm.data(node=fm.hexfunc(ctx.node()))
            cols = [
                ("filename", fn, True),
                ("rev", rev, True),
                ("linenumber", l.linenum, opts.get("line_number")),
            ]
            if opts.get("all"):
                cols.append(("change", change, True))
            cols.extend(
                [
                    ("user", formatuser(ctx.user()), opts.get("user")),
                    ("date", fm.formatdate(ctx.date(), datefmt), opts.get("date")),
                ]
            )
            lastcol = next(name for name, data, cond in reversed(cols) if cond)
            for name, data, cond in cols:
                field = fieldnamemap.get(name, name)
                fm.condwrite(cond, field, "%s", data, label="grep.%s" % name)
                if cond and name != lastcol:
                    fm.plain(sep, label="grep.sep")
            if not opts.get("files_with_matches"):
                fm.plain(sep, label="grep.sep")
                if not opts.get("text") and binary():
                    fm.plain(_(" Binary file matches"))
                else:
                    displaymatches(fm.nested("texts"), l)
            fm.plain(eol)
            found = True
            if opts.get("files_with_matches"):
                break
        return found

    def displaymatches(fm, l):
        p = 0
        for s, e in l.findpos():
            if p < s:
                fm.startitem()
                fm.write("text", "%s", l.line[p:s])
                fm.data(matched=False)
            fm.startitem()
            fm.write("text", "%s", l.line[s:e], label="grep.match")
            fm.data(matched=True)
            p = e
        if p < len(l.line):
            fm.startitem()
            fm.write("text", "%s", l.line[p:])
            fm.data(matched=False)
        fm.end()

    skip = {}
    revfiles = {}
    match = scmutil.match(repo[None], pats, opts)
    found = False
    follow = opts.get("follow")

    def prep(ctx, fns):
        rev = ctx.rev()
        pctx = ctx.p1()
        parent = pctx.rev()
        matches.setdefault(rev, {})
        matches.setdefault(parent, {})
        files = revfiles.setdefault(rev, [])
        for fn in fns:
            flog = getfile(fn)
            try:
                fnode = ctx.filenode(fn)
            except error.LookupError:
                continue

            copied = flog.renamed(fnode)
            copy = follow and copied and copied[0]
            if copy:
                copies.setdefault(rev, {})[fn] = copy
            if fn in skip:
                if copy:
                    skip[copy] = True
                continue
            files.append(fn)

            if fn not in matches[rev]:
                grepbody(fn, rev, flog.read(fnode))

            pfn = copy or fn
            if pfn not in matches[parent]:
                try:
                    fnode = pctx.filenode(pfn)
                    grepbody(pfn, parent, flog.read(fnode))
                except error.LookupError:
                    pass

    ui.pager("grep")
    fm = ui.formatter("grep", opts)
    for ctx in cmdutil.walkchangerevs(repo, match, opts, prep):
        rev = ctx.rev()
        parent = ctx.p1().rev()
        for fn in sorted(revfiles.get(rev, [])):
            states = matches[rev][fn]
            copy = copies.get(rev, {}).get(fn)
            if fn in skip:
                if copy:
                    skip[copy] = True
                continue
            pstates = matches.get(parent, {}).get(copy or fn, [])
            if pstates or states:
                r = display(fm, fn, ctx, pstates, states)
                found = found or r
                if r and not opts.get("all"):
                    skip[fn] = True
                    if copy:
                        skip[copy] = True
        del matches[rev]
        del revfiles[rev]
    fm.end()

    return not found


@command(
    "identify|id",
    [
        ("r", "rev", "", _("identify the specified revision"), _("REV")),
        ("n", "num", None, _("show local revision number")),
        ("i", "id", None, _("show global revision id")),
        ("b", "branch", None, _("print 'default' (DEPRECATED)")),
        ("t", "tags", None, _("show tags")),
        ("B", "bookmarks", None, _("show bookmarks")),
    ]
    + remoteopts
    + formatteropts,
    _("[-nibtB] [-r REV] [SOURCE]"),
    optionalrepo=True,
    cmdtype=readonly,
)
def identify(
    ui,
    repo,
    source=None,
    rev=None,
    num=None,
    id=None,
    branch=None,
    tags=None,
    bookmarks=None,
    **opts
):
    """identify the working directory or specified revision

    Print a summary identifying the repository state at REV using one or
    two parent hash identifiers, followed by a "+" if the working
    directory has uncommitted changes, a list of tags, and a list of bookmarks.

    When REV is not given, print a summary of the current state of the
    repository.

    Specifying a path to a repository root or Mercurial bundle will
    cause lookup to operate on that repository/bundle.

    .. container:: verbose

      Examples:

      - generate a build identifier for the working directory::

          hg id --id > build-id.dat

      - find the revision corresponding to a tag::

          hg id -n -r 1.3

      - check the most recent revision of a remote repository::

          hg id -r tip https://www.mercurial-scm.org/repo/hg/

    See :hg:`log` for generating more information about specific revisions,
    including full hash identifiers.

    Returns 0 if successful.
    """

    opts = pycompat.byteskwargs(opts)
    if not repo and not source:
        raise error.Abort(_("there is no Mercurial repository here " "(.hg not found)"))

    if ui.debugflag:
        hexfunc = hex
    else:
        hexfunc = short
    default = not (num or id or branch or tags or bookmarks)
    output = []
    revs = []

    if source:
        source, branches = hg.parseurl(ui.expandpath(source))
        peer = hg.peer(repo or ui, opts, source)  # only pass ui when no repo
        repo = peer.local()
        revs, checkout = hg.addbranchrevs(repo, peer, branches, None)

    fm = ui.formatter("identify", opts)
    fm.startitem()

    if not repo:
        if num or branch or tags:
            raise error.Abort(_("can't query remote revision number, branch, or tags"))
        if not rev and revs:
            rev = revs[0]
        if not rev:
            rev = "tip"

        remoterev = peer.lookup(rev)
        hexrev = hexfunc(remoterev)
        if default or id:
            output = [hexrev]
        fm.data(id=hexrev)

        def getbms():
            bms = []

            if "bookmarks" in peer.listkeys("namespaces"):
                hexremoterev = hex(remoterev)
                bms = [
                    bm
                    for bm, bmr in peer.listkeys("bookmarks").iteritems()
                    if bmr == hexremoterev
                ]

            return sorted(bms)

        bms = getbms()
        if bookmarks:
            output.extend(bms)
        elif default and not ui.quiet:
            # multiple bookmarks for a single parent separated by '/'
            bm = "/".join(bms)
            if bm:
                output.append(bm)

        fm.data(node=hex(remoterev))
        fm.data(bookmarks=fm.formatlist(bms, name="bookmark"))
    else:
        ctx = scmutil.revsingle(repo, rev, None)

        if ctx.rev() is None:
            ctx = repo[None]
            parents = ctx.parents()
            taglist = []
            for p in parents:
                taglist.extend(p.tags())

            dirty = ""
            if ctx.dirty(missing=True, merge=False, branch=False):
                dirty = "+"
            fm.data(dirty=dirty)

            hexoutput = [hexfunc(p.node()) for p in parents]
            if default or id:
                output = ["%s%s" % ("+".join(hexoutput), dirty)]
            fm.data(id="%s%s" % ("+".join(hexoutput), dirty))

            if num:
                numoutput = ["%d" % p.rev() for p in parents]
                output.append("%s%s" % ("+".join(numoutput), dirty))

            fn = fm.nested("parents")
            for p in parents:
                fn.startitem()
                fn.data(rev=p.rev())
                fn.data(node=p.hex())
                fn.context(ctx=p)
            fn.end()
        else:
            hexoutput = hexfunc(ctx.node())
            if default or id:
                output = [hexoutput]
            fm.data(id=hexoutput)

            if num:
                output.append(pycompat.bytestr(ctx.rev()))
            taglist = ctx.tags()

        if default and not ui.quiet:
            # multiple tags for a single parent separated by '/'
            t = "/".join(taglist)
            if t:
                output.append(t)

            # multiple bookmarks for a single parent separated by '/'
            bm = "/".join(ctx.bookmarks())
            if bm:
                output.append(bm)
        else:
            if branch:
                output.append(ctx.branch())

            if tags:
                output.extend(taglist)

            if bookmarks:
                output.extend(ctx.bookmarks())

        fm.data(node=ctx.hex())
        fm.data(tags=fm.formatlist(taglist, name="tag", sep=":"))
        fm.data(bookmarks=fm.formatlist(ctx.bookmarks(), name="bookmark"))
        fm.context(ctx=ctx)

    fm.plain("%s\n" % " ".join(output))
    fm.end()


@command(
    "import|patch",
    [
        (
            "p",
            "strip",
            1,
            _(
                "directory strip option for patch. This has the same "
                "meaning as the corresponding patch option"
            ),
            _("NUM"),
        ),
        ("b", "base", "", _("base path (DEPRECATED)"), _("PATH")),
        ("e", "edit", False, _("invoke editor on commit messages")),
        (
            "f",
            "force",
            None,
            _("skip check for outstanding uncommitted changes (DEPRECATED)"),
        ),
        ("", "no-commit", None, _("don't commit, just update the working directory")),
        ("", "bypass", None, _("apply patch without touching the working directory")),
        ("", "partial", None, _("commit even if some hunks fail")),
        ("", "exact", None, _("abort if patch would apply lossily")),
        ("", "prefix", "", _("apply patch to subdirectory"), _("DIR")),
    ]
    + commitopts
    + commitopts2
    + similarityopts,
    _("[OPTION]... PATCH..."),
)
def import_(ui, repo, patch1=None, *patches, **opts):
    """import an ordered set of patches

    Import a list of patches and commit them individually (unless
    --no-commit is specified).

    To read a patch from standard input (stdin), use "-" as the patch
    name. If a URL is specified, the patch will be downloaded from
    there.

    Import first applies changes to the working directory (unless
    --bypass is specified), import will abort if there are outstanding
    changes.

    Use --bypass to apply and commit patches directly to the
    repository, without affecting the working directory. Without
    --exact, patches will be applied on top of the working directory
    parent revision.

    You can import a patch straight from a mail message. Even patches
    as attachments work (to use the body part, it must have type
    text/plain or text/x-patch). From and Subject headers of email
    message are used as default committer and commit message. All
    text/plain body parts before first diff are added to the commit
    message.

    If the imported patch was generated by :hg:`export`, user and
    description from patch override values from message headers and
    body. Values given on command line with -m/--message and -u/--user
    override these.

    If --exact is specified, import will set the working directory to
    the parent of each patch before applying it, and will abort if the
    resulting changeset has a different ID than the one recorded in
    the patch. This will guard against various ways that portable
    patch formats and mail systems might fail to transfer Mercurial
    data or metadata. See :hg:`bundle` for lossless transmission.

    Use --partial to ensure a changeset will be created from the patch
    even if some hunks fail to apply. Hunks that fail to apply will be
    written to a <target-file>.rej file. Conflicts can then be resolved
    by hand before :hg:`commit --amend` is run to update the created
    changeset. This flag exists to let people import patches that
    partially apply without losing the associated metadata (author,
    date, description, ...).

    .. note::

       When no hunks apply cleanly, :hg:`import --partial` will create
       an empty changeset, importing only the patch metadata.

    With -s/--similarity, hg will attempt to discover renames and
    copies in the patch in the same way as :hg:`addremove`.

    It is possible to use external patch programs to perform the patch
    by setting the ``ui.patch`` configuration option. For the default
    internal tool, the fuzz can also be configured via ``patch.fuzz``.
    See :hg:`help config` for more information about configuration
    files and how to use these options.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    .. container:: verbose

      Examples:

      - import a traditional patch from a website and detect renames::

          hg import -s 80 http://example.com/bugfix.patch

      - import a changeset from an hgweb server::

          hg import https://www.mercurial-scm.org/repo/hg/rev/5ca8c111e9aa

      - import all the patches in an Unix-style mbox::

          hg import incoming-patches.mbox

      - import patches from stdin::

          hg import -

      - attempt to exactly restore an exported changeset (not always
        possible)::

          hg import --exact proposed-fix.patch

      - use an external tool to apply a patch which is too fuzzy for
        the default internal tool.

          hg import --config ui.patch="patch --merge" fuzzy.patch

      - change the default fuzzing from 2 to a less strict 7

          hg import --config ui.fuzz=7 fuzz.patch

    Returns 0 on success, 1 on partial success (see --partial).
    """

    opts = pycompat.byteskwargs(opts)
    if not patch1:
        raise error.Abort(_("need at least one patch to import"))

    patches = (patch1,) + patches

    date = opts.get("date")
    if date:
        opts["date"] = util.parsedate(date)

    exact = opts.get("exact")
    update = not opts.get("bypass")
    if not update and opts.get("no_commit"):
        raise error.Abort(_("cannot use --no-commit with --bypass"))
    try:
        sim = float(opts.get("similarity") or 0)
    except ValueError:
        raise error.Abort(_("similarity must be a number"))
    if sim < 0 or sim > 100:
        raise error.Abort(_("similarity must be between 0 and 100"))
    if sim and not update:
        raise error.Abort(_("cannot use --similarity with --bypass"))
    if exact:
        if opts.get("edit"):
            raise error.Abort(_("cannot use --exact with --edit"))
        if opts.get("prefix"):
            raise error.Abort(_("cannot use --exact with --prefix"))

    base = opts["base"]
    wlock = dsguard = lock = tr = None
    msgs = []
    ret = 0

    try:
        wlock = repo.wlock()

        if update:
            cmdutil.checkunfinished(repo)
            if exact or not opts.get("force"):
                cmdutil.bailifchanged(repo)

        if not opts.get("no_commit"):
            lock = repo.lock()
            tr = repo.transaction("import")
        else:
            dsguard = dirstateguard.dirstateguard(repo, "import")
        parents = repo[None].parents()
        for patchurl in patches:
            if patchurl == "-":
                ui.status(_("applying patch from stdin\n"))
                patchfile = ui.fin
                patchurl = "stdin"  # for error message
            else:
                patchurl = os.path.join(base, patchurl)
                ui.status(_("applying %s\n") % patchurl)
                patchfile = hg.openpath(ui, patchurl)

            haspatch = False
            for hunk in patch.split(patchfile):
                (msg, node, rej) = cmdutil.tryimportone(
                    ui, repo, hunk, parents, opts, msgs, hg.clean
                )
                if msg:
                    haspatch = True
                    ui.note(msg + "\n")
                if update or exact:
                    parents = repo[None].parents()
                else:
                    parents = [repo[node]]
                if rej:
                    ui.write_err(_("patch applied partially\n"))
                    ui.write_err(
                        _("(fix the .rej files and run " "`hg commit --amend`)\n")
                    )
                    ret = 1
                    break

            if not haspatch:
                raise error.Abort(_("%s: no diffs found") % patchurl)

        if tr:
            tr.close()
        if msgs:
            repo.savecommitmessage("\n* * *\n".join(msgs))
        if dsguard:
            dsguard.close()
        return ret
    finally:
        if tr:
            tr.release()
        release(lock, dsguard, wlock)


@command(
    "incoming|in",
    [
        ("f", "force", None, _("run even if remote repository is unrelated")),
        ("n", "newest-first", None, _("show newest record first")),
        ("", "bundle", "", _("file to store the bundles into"), _("FILE")),
        ("r", "rev", [], _("a remote changeset intended to be added"), _("REV")),
        ("B", "bookmarks", False, _("compare bookmarks")),
    ]
    + logopts
    + remoteopts,
    _("[-p] [-n] [-M] [-f] [-r REV]... [--bundle FILENAME] [SOURCE]"),
)
def incoming(ui, repo, source="default", **opts):
    """show new changesets found in source

    Show new changesets found in the specified path/URL or the default
    pull location. These are the changesets that would have been pulled
    by :hg:`pull` at the time you issued this command.

    See pull for valid source format details.

    .. container:: verbose

      With -B/--bookmarks, the result of bookmark comparison between
      local and remote repositories is displayed. With -v/--verbose,
      status is also displayed for each bookmark like below::

        BM1               01234567890a added
        BM2               1234567890ab advanced
        BM3               234567890abc diverged
        BM4               34567890abcd changed

      The action taken locally when pulling depends on the
      status of each bookmark:

      :``added``: pull will create it
      :``advanced``: pull will update it
      :``diverged``: pull will create a divergent bookmark
      :``changed``: result depends on remote changesets

      From the point of view of pulling behavior, bookmark
      existing only in the remote repository are treated as ``added``,
      even if it is in fact locally deleted.

    .. container:: verbose

      For remote repository, using --bundle avoids downloading the
      changesets twice if the incoming is followed by a pull.

      Examples:

      - show incoming changes with patches and full description::

          hg incoming -vp

      - show incoming changes excluding merges, store a bundle::

          hg in -vpM --bundle incoming.hg
          hg pull incoming.hg

      - briefly list changes inside a bundle::

          hg in changes.hg -T "{desc|firstline}\\n"

    Returns 0 if there are incoming changes, 1 otherwise.

    Raises an error if enableincomingoutgoing is set to FALSE.
    """

    if not ui.configbool("ui", "enableincomingoutgoing"):
        raise error.Abort(_("incoming is not supported for this repository"))
    opts = pycompat.byteskwargs(opts)
    if opts.get("graph"):
        cmdutil.checkunsupportedgraphflags([], opts)

        def display(other, chlist, displayer):
            revdag = cmdutil.graphrevs(other, chlist, opts)
            cmdutil.displaygraph(ui, repo, revdag, displayer, graphmod.asciiedges)

        hg._incoming(display, ui, repo, source, opts, buffered=True)
        return 0

    if opts.get("bookmarks"):
        source, branches = hg.parseurl(ui.expandpath(source))
        other = hg.peer(repo, opts, source)
        if "bookmarks" not in other.listkeys("namespaces"):
            ui.warn(_("remote doesn't support bookmarks\n"))
            return 0
        ui.pager("incoming")
        ui.status(_("comparing with %s\n") % util.hidepassword(source))
        return bookmarks.incoming(ui, repo, other)

    repo._subtoppath = ui.expandpath(source)
    try:
        return hg.incoming(ui, repo, source, opts)
    finally:
        del repo._subtoppath


@command("^init", remoteopts, _("[-e CMD] [--remotecmd CMD] [DEST]"), norepo=True)
def init(ui, dest=".", **opts):
    """create a new repository in the given directory

    Initialize a new repository in the given directory. If the given
    directory does not exist, it will be created.

    If no directory is given, the current directory is used.

    It is possible to specify an ``ssh://`` URL as the destination.
    See :hg:`help urls` for more information.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    hg.peer(ui, opts, ui.expandpath(dest), create=True)


@command(
    "locate",
    [
        ("r", "rev", "", _("search the repository as it is in REV"), _("REV")),
        ("0", "print0", None, _("end filenames with NUL, for use with xargs")),
        ("f", "fullpath", None, _("print complete paths from the filesystem root")),
    ]
    + walkopts,
    _("[OPTION]... [PATTERN]..."),
)
def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns (DEPRECATED)

    Print files under Mercurial control in the working directory whose
    names match the given patterns.

    By default, this command searches all directories in the working
    directory. To search just the current directory and its
    subdirectories, use "--include .".

    If no patterns are given to match, this command prints the names
    of all files under Mercurial control in the working directory.

    If you want to feed the output of this command into the "xargs"
    command, use the -0 option to both this command and "xargs". This
    will avoid the problem of "xargs" treating single filenames that
    contain whitespace as multiple filenames.

    See :hg:`help files` for a more versatile command.

    Returns 0 if a match is found, 1 otherwise.
    """
    opts = pycompat.byteskwargs(opts)
    if opts.get("print0"):
        end = "\0"
    else:
        end = "\n"
    rev = scmutil.revsingle(repo, opts.get("rev"), None).node()

    ret = 1
    ctx = repo[rev]
    m = scmutil.match(ctx, pats, opts, default="relglob", badfn=lambda x, y: False)

    ui.pager("locate")
    for abs in ctx.matches(m):
        if opts.get("fullpath"):
            ui.write(repo.wjoin(abs), end)
        else:
            ui.write(((pats and m.rel(abs)) or abs), end)
        ret = 0

    return ret


@command(
    "^log|history",
    [
        (
            "f",
            "follow",
            None,
            _("follow changeset history, or file history across copies and renames"),
        ),
        (
            "",
            "follow-first",
            None,
            _("only follow the first parent of merge changesets (DEPRECATED)"),
        ),
        ("d", "date", "", _("show revisions matching date spec"), _("DATE")),
        ("C", "copies", None, _("show copied files")),
        (
            "k",
            "keyword",
            [],
            _("do case-insensitive search for a given text"),
            _("TEXT"),
        ),
        ("r", "rev", [], _("show the specified revision or revset"), _("REV")),
        (
            "L",
            "line-range",
            [],
            _("follow line range of specified file (EXPERIMENTAL)"),
            _("FILE,RANGE"),
        ),
        ("", "removed", None, _("include revisions where files were removed")),
        ("m", "only-merges", None, _("show only merges (DEPRECATED)")),
        ("u", "user", [], _("revisions committed by user"), _("USER")),
        (
            "b",
            "branch",
            [],
            _("show changesets within the given named branch"),
            _("BRANCH"),
        ),
        (
            "P",
            "prune",
            [],
            _("do not display revision or any of its ancestors"),
            _("REV"),
        ),
    ]
    + logopts
    + walkopts,
    _("[OPTION]... [FILE]"),
    inferrepo=True,
    cmdtype=readonly,
)
def log(ui, repo, *pats, **opts):
    """show commit history

    Print the revision history of the specified files or the entire
    project.

    If no revision range is specified, the default is ``tip:0`` unless
    --follow is set, in which case the working directory parent is
    used as the starting revision.

    File history is shown without following rename or copy history of
    files. Use -f/--follow with a filename to follow history across
    renames and copies. --follow without a filename will only show
    ancestors or descendants of the starting revision.

    By default this command prints revision number and changeset id,
    tags, non-trivial parents, user, date and time, and a summary for
    each commit. When the -v/--verbose switch is used, the list of
    changed files and full commit message are shown.

    With --graph the revisions are shown as an ASCII art DAG with the most
    recent changeset at the top.
    'o' is a changeset, '@' is a working directory parent, 'x' is obsolete,
    and '+' represents a fork where the changeset from the lines below is a
    parent of the 'o' merge on the same line.
    Paths in the DAG are represented with '|', '/' and so forth. ':' in place
    of a '|' indicates one or more revisions in a path are omitted.

    .. container:: verbose

       Use -L/--line-range FILE,M:N options to follow the history of lines
       from M to N in FILE. With -p/--patch only diff hunks affecting
       specified line range will be shown. This option requires --follow;
       it can be specified multiple times. Currently, this option is not
       compatible with --graph. This option is experimental.

    .. note::

       :hg:`log --patch` may generate unexpected diff output for merge
       changesets, as it will only compare the merge changeset against
       its first parent. Also, only files different from BOTH parents
       will appear in files:.

    .. note::

       For performance reasons, :hg:`log FILE` may omit duplicate changes
       made on branches and will not show removals or mode changes. To
       see all such changes, use the --removed switch.

    .. container:: verbose

       .. note::

          The history resulting from -L/--line-range options depends on diff
          options; for instance if white-spaces are ignored, respective changes
          with only white-spaces in specified line range will not be listed.

    .. container:: verbose

      Some examples:

      - changesets with full descriptions and file lists::

          hg log -v

      - changesets ancestral to the working directory::

          hg log -f

      - last 10 commits on the current branch::

          hg log -l 10 -b .

      - changesets showing all modifications of a file, including removals::

          hg log --removed file.c

      - all changesets that touch a directory, with diffs, excluding merges::

          hg log -Mp lib/

      - all revision numbers that match a keyword::

          hg log -k bug --template "{rev}\\n"

      - the full hash identifier of the working directory parent::

          hg log -r . --template "{node}\\n"

      - list available log templates::

          hg log -T list

      - check if a given changeset is included in a tagged release::

          hg log -r "a21ccf and ancestor(1.9)"

      - find all changesets by some user in a date range::

          hg log -k alice -d "may 2008 to jul 2008"

      - summary of all changesets after the last tag::

          hg log -r "last(tagged())::" --template "{desc|firstline}\\n"

      - changesets touching lines 13 to 23 for file.c::

          hg log -L file.c,13:23

      - changesets touching lines 13 to 23 for file.c and lines 2 to 6 of
        main.c with patch::

          hg log -L file.c,13:23 -L main.c,2:6 -p

    See :hg:`help dates` for a list of formats valid for -d/--date.

    See :hg:`help revisions` for more about specifying and ordering
    revisions.

    See :hg:`help templates` for more about pre-packaged styles and
    specifying custom templates. The default template used by the log
    command can be customized via the ``ui.logtemplate`` configuration
    setting.

    Returns 0 on success.

    """
    opts = pycompat.byteskwargs(opts)
    linerange = opts.get("line_range")

    if linerange and not opts.get("follow"):
        raise error.Abort(_("--line-range requires --follow"))

    if linerange and pats:
        raise error.Abort(
            _("FILE arguments are not compatible with --line-range option")
        )

    if opts.get("follow") and opts.get("rev"):
        opts["rev"] = [revsetlang.formatspec("reverse(::%lr)", opts.get("rev"))]
        del opts["follow"]

    if opts.get("graph"):
        if linerange:
            raise error.Abort(_("graph not supported with line range patterns"))
        return cmdutil.graphlog(ui, repo, pats, opts)

    revs, expr, filematcher = cmdutil.getlogrevs(repo, pats, opts)
    hunksfilter = None

    if linerange:
        revs, lrfilematcher, hunksfilter = cmdutil.getloglinerangerevs(repo, revs, opts)

        if filematcher is not None and lrfilematcher is not None:
            basefilematcher = filematcher

            def filematcher(rev):
                files = basefilematcher(rev).files() + lrfilematcher(rev).files()
                return scmutil.matchfiles(repo, files)

        elif filematcher is None:
            filematcher = lrfilematcher

    limit = cmdutil.loglimit(opts)
    count = 0

    getrenamed = None
    if opts.get("copies"):
        endrev = None
        if opts.get("rev"):
            endrev = scmutil.revrange(repo, opts.get("rev")).max() + 1
        getrenamed = templatekw.getrenamedfn(repo, endrev=endrev)

    ui.pager("log")
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    for rev in revs:
        if count == limit:
            break
        ctx = repo[rev]
        copies = None
        if getrenamed is not None and rev:
            copies = []
            for fn in ctx.files():
                rename = getrenamed(fn, rev)
                if rename:
                    copies.append((fn, rename[0]))
        if filematcher:
            revmatchfn = filematcher(ctx.rev())
        else:
            revmatchfn = None
        if hunksfilter:
            revhunksfilter = hunksfilter(rev)
        else:
            revhunksfilter = None
        displayer.show(
            ctx, copies=copies, matchfn=revmatchfn, hunksfilterfn=revhunksfilter
        )
        if displayer.flush(ctx):
            count += 1

    displayer.close()


@command(
    "manifest",
    [
        ("r", "rev", "", _("revision to display"), _("REV")),
        ("", "all", False, _("list files from all revisions")),
    ]
    + formatteropts,
    _("[-r REV]"),
    cmdtype=readonly,
)
def manifest(ui, repo, node=None, rev=None, **opts):
    """output the current or given revision of the project manifest

    Print a list of version controlled files for the given revision.
    If no revision is given, the first parent of the working directory
    is used, or the null revision if no revision is checked out.

    With -v, print file permissions, symlink and executable bits.
    With --debug, print file revision hashes.

    If option --all is specified, the list of all files from all revisions
    is printed. This includes deleted and renamed files.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    fm = ui.formatter("manifest", opts)

    if opts.get("all"):
        if rev or node:
            raise error.Abort(_("can't specify a revision with --all"))

        res = []
        prefix = "data/"
        suffix = ".i"
        plen = len(prefix)
        slen = len(suffix)
        with repo.lock():
            for fn, b, size in repo.store.datafiles():
                if size != 0 and fn[-slen:] == suffix and fn[:plen] == prefix:
                    res.append(fn[plen:-slen])
        ui.pager("manifest")
        for f in res:
            fm.startitem()
            fm.write("path", "%s\n", f)
        fm.end()
        return

    if rev and node:
        raise error.Abort(_("please specify just one revision"))

    if not node:
        node = rev

    char = {"l": "@", "x": "*", "": ""}
    mode = {"l": "644", "x": "755", "": "644"}
    ctx = scmutil.revsingle(repo, node)
    mf = ctx.manifest()
    ui.pager("manifest")
    for f in ctx:
        fm.startitem()
        fl = ctx[f].flags()
        fm.condwrite(ui.debugflag, "hash", "%s ", hex(mf[f]))
        fm.condwrite(ui.verbose, "mode type", "%s %1s ", mode[fl], char[fl])
        fm.write("path", "%s\n", f)
    fm.end()


@command(
    "^merge",
    [
        (
            "f",
            "force",
            None,
            _("force a merge including outstanding changes (DEPRECATED)"),
        ),
        ("r", "rev", "", _("revision to merge"), _("REV")),
        ("P", "preview", None, _("review revisions to merge (no merge is performed)")),
    ]
    + mergetoolopts,
    _("[-P] [[-r] REV]"),
)
def merge(ui, repo, node=None, **opts):
    """merge another revision into working directory

    The current working directory is updated with all changes made in
    the requested revision since the last common predecessor revision.

    Files that changed between either parent are marked as changed for
    the next commit and a commit must be performed before any further
    updates to the repository are allowed. The next commit will have
    two parents.

    ``--tool`` can be used to specify the merge tool used for file
    merges. It overrides the HGMERGE environment variable and your
    configuration files. See :hg:`help merge-tools` for options.

    If no revision is specified, the working directory's parent is a
    head revision, and the current branch contains exactly one other
    head, the other head is merged with by default. Otherwise, an
    explicit revision with which to merge with must be provided.

    See :hg:`help resolve` for information on handling file conflicts.

    To undo an uncommitted merge, use :hg:`update --clean .` which
    will check out a clean copy of the original merge parent, losing
    all changes.

    .. container:: verbose

      The merge command can be entirely disabled by setting the
      ``ui.allowmerge`` configuration setting to false.

    Returns 0 on success, 1 if there are unresolved files.
    """
    if not ui.configbool("ui", "allowmerge", default=True):
        raise error.Abort(
            _("merging is not supported for this repository"),
            hint=_("use rebase instead"),
        )
    opts = pycompat.byteskwargs(opts)
    if opts.get("rev") and node:
        raise error.Abort(_("please specify just one revision"))
    if not node:
        node = opts.get("rev")

    if node:
        node = scmutil.revsingle(repo, node).node()

    if not node:
        node = repo[destutil.destmerge(repo)].node()

    if opts.get("preview"):
        # find nodes that are ancestors of p2 but not of p1
        p1 = repo.lookup(".")
        p2 = repo.lookup(node)
        nodes = repo.changelog.findmissing(common=[p1], heads=[p2])

        displayer = cmdutil.show_changeset(ui, repo, opts)
        for node in nodes:
            displayer.show(repo[node])
        displayer.close()
        return 0

    try:
        # ui.forcemerge is an internal variable, do not document
        repo.ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "merge")
        force = opts.get("force")
        labels = ["working copy", "merge rev"]
        return hg.merge(repo, node, force=force, mergeforce=force, labels=labels)
    finally:
        ui.setconfig("ui", "forcemerge", "", "merge")


@command(
    "outgoing|out",
    [
        ("f", "force", None, _("run even when the destination is unrelated")),
        (
            "r",
            "rev",
            [],
            _("a changeset intended to be included in the destination"),
            _("REV"),
        ),
        ("n", "newest-first", None, _("show newest record first")),
        ("B", "bookmarks", False, _("compare bookmarks")),
    ]
    + logopts
    + remoteopts,
    _("[-M] [-p] [-n] [-f] [-r REV]... [DEST]"),
)
def outgoing(ui, repo, dest=None, **opts):
    """show changesets not found in the destination

    Show changesets not found in the specified destination repository
    or the default push location. These are the changesets that would
    be pushed if a push was requested.

    See pull for details of valid destination formats.

    .. container:: verbose

      With -B/--bookmarks, the result of bookmark comparison between
      local and remote repositories is displayed. With -v/--verbose,
      status is also displayed for each bookmark like below::

        BM1               01234567890a added
        BM2                            deleted
        BM3               234567890abc advanced
        BM4               34567890abcd diverged
        BM5               4567890abcde changed

      The action taken when pushing depends on the
      status of each bookmark:

      :``added``: push with ``-B`` will create it
      :``deleted``: push with ``-B`` will delete it
      :``advanced``: push will update it
      :``diverged``: push with ``-B`` will update it
      :``changed``: push with ``-B`` will update it

      From the point of view of pushing behavior, bookmarks
      existing only in the remote repository are treated as
      ``deleted``, even if it is in fact added remotely.

    Returns 0 if there are outgoing changes, 1 otherwise.
    """
    if not ui.configbool("ui", "enableincomingoutgoing"):
        raise error.Abort(_("outgoing is not supported for this repository"))

    opts = pycompat.byteskwargs(opts)
    if opts.get("graph"):
        cmdutil.checkunsupportedgraphflags([], opts)
        o, other = hg._outgoing(ui, repo, dest, opts)
        if not o:
            cmdutil.outgoinghooks(ui, repo, other, opts, o)
            return

        revdag = cmdutil.graphrevs(repo, o, opts)
        ui.pager("outgoing")
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        cmdutil.displaygraph(ui, repo, revdag, displayer, graphmod.asciiedges)
        cmdutil.outgoinghooks(ui, repo, other, opts, o)
        return 0

    if opts.get("bookmarks"):
        dest = ui.expandpath(dest or "default-push", dest or "default")
        dest, branches = hg.parseurl(dest)
        other = hg.peer(repo, opts, dest)
        if "bookmarks" not in other.listkeys("namespaces"):
            ui.warn(_("remote doesn't support bookmarks\n"))
            return 0
        ui.status(_("comparing with %s\n") % util.hidepassword(dest))
        ui.pager("outgoing")
        return bookmarks.outgoing(ui, repo, other)

    repo._subtoppath = ui.expandpath(dest or "default-push", dest or "default")
    try:
        return hg.outgoing(ui, repo, dest, opts)
    finally:
        del repo._subtoppath


@command(
    "parents",
    [("r", "rev", "", _("show parents of the specified revision"), _("REV"))]
    + templateopts,
    _("[-r REV] [FILE]"),
    inferrepo=True,
)
def parents(ui, repo, file_=None, **opts):
    """show the parents of the working directory or revision (DEPRECATED)

    Print the working directory's parent revisions. If a revision is
    given via -r/--rev, the parent of that revision will be printed.
    If a file argument is given, the revision in which the file was
    last changed (before the working directory revision or the
    argument to --rev if given) is printed.

    This command is equivalent to::

        hg log -r "p1()+p2()" or
        hg log -r "p1(REV)+p2(REV)" or
        hg log -r "max(::p1() and file(FILE))+max(::p2() and file(FILE))" or
        hg log -r "max(::p1(REV) and file(FILE))+max(::p2(REV) and file(FILE))"

    See :hg:`summary` and :hg:`help revsets` for related information.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    ctx = scmutil.revsingle(repo, opts.get("rev"), None)

    if file_:
        m = scmutil.match(ctx, (file_,), opts)
        if m.anypats() or len(m.files()) != 1:
            raise error.Abort(_("can only specify an explicit filename"))
        file_ = m.files()[0]
        filenodes = []
        for cp in ctx.parents():
            if not cp:
                continue
            try:
                filenodes.append(cp.filenode(file_))
            except error.LookupError:
                pass
        if not filenodes:
            raise error.Abort(_("'%s' not found in manifest!") % file_)
        p = []
        for fn in filenodes:
            fctx = repo.filectx(file_, fileid=fn)
            p.append(fctx.node())
    else:
        p = [cp.node() for cp in ctx.parents()]

    displayer = cmdutil.show_changeset(ui, repo, opts)
    for n in p:
        if n != nullid:
            displayer.show(repo[n])
    displayer.close()


@command("paths", formatteropts, _("[NAME]"), optionalrepo=True, cmdtype=readonly)
def paths(ui, repo, search=None, **opts):
    """show aliases for remote repositories

    Show definition of symbolic path name NAME. If no name is given,
    show definition of all available names.

    Option -q/--quiet suppresses all output when searching for NAME
    and shows only the path names when listing all definitions.

    Path names are defined in the [paths] section of your
    configuration file and in ``/etc/mercurial/hgrc``. If run inside a
    repository, ``.hg/hgrc`` is used, too.

    The path names ``default`` and ``default-push`` have a special
    meaning.  When performing a push or pull operation, they are used
    as fallbacks if no location is specified on the command-line.
    When ``default-push`` is set, it will be used for push and
    ``default`` will be used for pull; otherwise ``default`` is used
    as the fallback for both.  When cloning a repository, the clone
    source is written as ``default`` in ``.hg/hgrc``.

    .. note::

       ``default`` and ``default-push`` apply to all inbound (e.g.
       :hg:`incoming`) and outbound (e.g. :hg:`outgoing`, :hg:`email`
       and :hg:`bundle`) operations.

    See :hg:`help urls` for more information.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    ui.pager("paths")
    if search:
        pathitems = [
            (name, path) for name, path in ui.paths.iteritems() if name == search
        ]
    else:
        pathitems = sorted(ui.paths.iteritems())

    fm = ui.formatter("paths", opts)
    if fm.isplain():
        hidepassword = util.hidepassword
    else:
        hidepassword = str
    if ui.quiet:
        namefmt = "%s\n"
    else:
        namefmt = "%s = "
    showsubopts = not search and not ui.quiet

    for name, path in pathitems:
        fm.startitem()
        fm.condwrite(not search, "name", namefmt, name)
        fm.condwrite(not ui.quiet, "url", "%s\n", hidepassword(path.rawloc))
        for subopt, value in sorted(path.suboptions.items()):
            assert subopt not in ("name", "url")
            if showsubopts:
                fm.plain("%s:%s = " % (name, subopt))
            fm.condwrite(showsubopts, subopt, "%s\n", value)

    fm.end()

    if search and not pathitems:
        if not ui.quiet:
            ui.warn(_("not found!\n"))
        return 1
    else:
        return 0


@command(
    "phase",
    [
        ("p", "public", False, _("set changeset phase to public")),
        ("d", "draft", False, _("set changeset phase to draft")),
        ("s", "secret", False, _("set changeset phase to secret")),
        ("f", "force", False, _("allow to move boundary backward")),
        ("r", "rev", [], _("target revision"), _("REV")),
    ],
    _("[-p|-d|-s] [-f] [-r] [REV...]"),
)
def phase(ui, repo, *revs, **opts):
    """set or show the current phase name

    With no argument, show the phase name of the current revision(s).

    With one of -p/--public, -d/--draft or -s/--secret, change the
    phase value of the specified revisions.

    Unless -f/--force is specified, :hg:`phase` won't move changesets from a
    lower phase to a higher phase. Phases are ordered as follows::

        public < draft < secret

    Returns 0 on success, 1 if some phases could not be changed.

    (For more information about the phases concept, see :hg:`help phases`.)
    """
    opts = pycompat.byteskwargs(opts)
    # search for a unique phase argument
    targetphase = None
    for idx, name in enumerate(phases.phasenames):
        if opts[name]:
            if targetphase is not None:
                raise error.Abort(_("only one phase can be specified"))
            targetphase = idx

    # look for specified revision
    revs = list(revs)
    revs.extend(opts["rev"])
    if not revs:
        # display both parents as the second parent phase can influence
        # the phase of a merge commit
        revs = [c.rev() for c in repo[None].parents()]

    revs = scmutil.revrange(repo, revs)

    lock = None
    ret = 0
    if targetphase is None:
        # display
        for r in revs:
            ctx = repo[r]
            ui.write("%i: %s\n" % (ctx.rev(), ctx.phasestr()))
    else:
        tr = None
        lock = repo.lock()
        try:
            tr = repo.transaction("phase")
            # set phase
            if not revs:
                raise error.Abort(_("empty revision set"))
            nodes = [repo[r].node() for r in revs]

            phases.advanceboundary(repo, tr, targetphase, nodes)
            if opts["force"]:
                phases.retractboundary(repo, tr, targetphase, nodes)
            tr.close()
        finally:
            if tr is not None:
                tr.release()
            lock.release()

        # moving revision from public to draft may hide them
        # We have to check result on an unfiltered repository
        unfi = repo.unfiltered()
        cl = unfi.changelog
        getphase = unfi._phasecache.phase
        rejected = [n for n in nodes if getphase(unfi, cl.rev(n)) < targetphase]
        if rejected:
            ui.warn(
                _("cannot move %i changesets to a higher " "phase, use --force\n")
                % len(rejected)
            )
            ret = 1
    return ret


def postincoming(ui, repo, modheads, optupdate, checkout, brev):
    """Run after a changegroup has been added via pull/unbundle

    This takes arguments below:

    :modheads: change of heads by pull/unbundle
    :optupdate: updating working directory is needed or not
    :checkout: update destination revision (or None to default destination)
    :brev: a name, which might be a bookmark to be activated after updating
    """
    if modheads == 0:
        return
    if optupdate:
        try:
            return hg.updatetotally(ui, repo, checkout, brev)
        except error.UpdateAbort as inst:
            msg = _("not updating: %s") % str(inst)
            hint = inst.hint
            raise error.UpdateAbort(msg, hint=hint)


@command(
    "^pull",
    [
        (
            "u",
            "update",
            None,
            _("update to new branch head if new descendants were pulled"),
        ),
        ("f", "force", None, _("run even when remote repository is unrelated")),
        ("r", "rev", [], _("a remote commit to pull"), _("REV")),
        ("B", "bookmark", [], _("a bookmark to pull"), _("BOOKMARK")),
    ]
    + remoteopts,
    _("[-u] [-f] [-r REV]... [-e CMD] [--remotecmd CMD] [SOURCE]"),
)
def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source

    Pull changes from a remote repository to a local one. This command modifies
    the commit graph, but doesn't affect local commits or files on disk.

    If SOURCE is omitted, the default path is used.
    See :hg:`help urls` for more information.

    .. container:: verbose

        You can use ``.`` for BOOKMARK to specify the active bookmark.

    Returns 0 on success, 1 if ``--update`` was specified but the update had unresolved conflicts.
    """
    if ui.configbool("pull", "automigrate"):
        repo.automigrate()

    opts = pycompat.byteskwargs(opts)
    if ui.configbool("commands", "update.requiredest") and opts.get("update"):
        msg = _("update destination required by configuration")
        hint = _("use hg pull followed by hg update DEST")
        raise error.Abort(msg, hint=hint)

    source, branches = hg.parseurl(ui.expandpath(source))
    ui.status(_("pulling from %s\n") % util.hidepassword(source))

    with repo.connectionpool.get(source, opts=opts) as conn:
        other = conn.peer
        revs, checkout = hg.addbranchrevs(repo, other, branches, opts.get("rev"))

        pullopargs = {}
        if opts.get("bookmark"):
            if not revs:
                revs = []
            # The list of bookmark used here is not the one used to actually
            # update the bookmark name. This can result in the revision pulled
            # not ending up with the name of the bookmark because of a race
            # condition on the server. (See issue 4689 for details)
            remotebookmarks = other.listkeys("bookmarks")
            remotebookmarks = bookmarks.unhexlifybookmarks(remotebookmarks)
            pullopargs["remotebookmarks"] = remotebookmarks
            for b in opts["bookmark"]:
                b = repo._bookmarks.expandname(b)
                if b not in remotebookmarks:
                    raise error.Abort(_("remote bookmark %s not found!") % b)
                revs.append(hex(remotebookmarks[b]))

        if revs:
            try:
                # When 'rev' is a bookmark name, we cannot guarantee that it
                # will be updated with that name because of a race condition
                # server side. (See issue 4689 for details)
                oldrevs = revs
                revs = []  # actually, nodes
                for r in oldrevs:
                    node = other.lookup(r)
                    revs.append(node)
                    if r == checkout:
                        checkout = node
            except error.CapabilityError:
                err = _(
                    "other repository doesn't support revision lookup, "
                    "so a rev cannot be specified."
                )
                raise error.Abort(err)

        pullopargs.update(opts.get("opargs", {}))
        modheads = exchange.pull(
            repo,
            other,
            heads=revs,
            force=opts.get("force"),
            bookmarks=opts.get("bookmark", ()),
            opargs=pullopargs,
        ).cgresult

        # brev is a name, which might be a bookmark to be activated at
        # the end of the update. In other words, it is an explicit
        # destination of the update
        brev = None

        if checkout:
            checkout = str(repo.changelog.rev(checkout))

            # order below depends on implementation of
            # hg.addbranchrevs(). opts['bookmark'] is ignored,
            # because 'checkout' is determined without it.
            if opts.get("rev"):
                brev = opts["rev"][0]
            else:
                brev = branches[0]
        repo._subtoppath = source
        try:
            ret = postincoming(ui, repo, modheads, opts.get("update"), checkout, brev)

        finally:
            del repo._subtoppath

    return ret


@command(
    "^push",
    [
        ("f", "force", None, _("force push")),
        (
            "r",
            "rev",
            [],
            _("a changeset intended to be included in the destination"),
            _("REV"),
        ),
        ("B", "bookmark", [], _("bookmark to push"), _("BOOKMARK")),
        ("", "new-branch", False, _("allow pushing a new branch (DEPRECATED)")),
        ("", "pushvars", [], _("variables that can be sent to server (ADVANCED)")),
    ]
    + remoteopts,
    _("[-f] [-r REV]... [-e CMD] [--remotecmd CMD] [DEST]"),
)
def push(ui, repo, dest=None, **opts):
    """push changes to the specified destination

    Push changesets from the local repository to the specified
    destination.

    This operation is symmetrical to pull: it is identical to a pull
    in the destination repository from the current one.

    By default, push will not allow creation of new heads at the
    destination, since multiple heads would make it unclear which head
    to use. In this situation, it is recommended to pull and merge
    before pushing.

    .. note::

       Extra care should be taken with the -f/--force option,
       which will push all new heads on all branches, an action which will
       almost always cause confusion for collaborators.

    If -r/--rev is used, the specified revision and all its ancestors
    will be pushed to the remote repository.

    If -B/--bookmark is used, the specified bookmarked revision, its
    ancestors, and the bookmark will be pushed to the remote
    repository. Specifying ``.`` is equivalent to specifying the active
    bookmark's name.

    Please see :hg:`help urls` for important details about ``ssh://``
    URLs. If DESTINATION is omitted, a default path will be used.

    .. container:: verbose

        The --pushvars option sends strings to the server that become
        environment variables prepended with ``HG_USERVAR_``. For example,
        ``--pushvars ENABLE_FEATURE=true``, provides the server side hooks with
        ``HG_USERVAR_ENABLE_FEATURE=true`` as part of their environment.

        pushvars can provide for user-overridable hooks as well as set debug
        levels. One example is having a hook that blocks commits containing
        conflict markers, but enables the user to override the hook if the file
        is using conflict markers for testing purposes or the file format has
        strings that look like conflict markers.

        By default, servers will ignore `--pushvars`. To enable it add the
        following to your configuration file::

            [push]
            pushvars.server = true

    Returns 0 if push was successful, 1 if nothing to push.
    """

    opts = pycompat.byteskwargs(opts)
    if opts.get("bookmark"):
        ui.setconfig("bookmarks", "pushing", opts["bookmark"], "push")
        for b in opts["bookmark"]:
            # translate -B options to -r so changesets get pushed
            b = repo._bookmarks.expandname(b)
            if b in repo._bookmarks:
                opts.setdefault("rev", []).append(b)
            else:
                # if we try to push a deleted bookmark, translate it to null
                # this lets simultaneous -r, -b options continue working
                opts.setdefault("rev", []).append("null")

    path = ui.paths.getpath(dest, default=("default-push", "default"))
    if not path:
        raise error.Abort(
            _("default repository not configured!"),
            hint=_("see 'hg help config.paths'"),
        )
    dest = path.pushloc or path.loc
    branches = (path.branch, [])
    ui.status(_("pushing to %s\n") % util.hidepassword(dest))
    revs, checkout = hg.addbranchrevs(repo, repo, branches, opts.get("rev"))
    other = hg.peer(repo, opts, dest)

    if revs:
        revs = [repo.lookup(r) for r in scmutil.revrange(repo, revs)]
        if not revs:
            raise error.Abort(
                _("specified revisions evaluate to an empty set"),
                hint=_("use different revision arguments"),
            )
    elif path.pushrev:
        # It doesn't make any sense to specify ancestor revisions. So limit
        # to DAG heads to make discovery simpler.
        expr = revsetlang.formatspec("heads(%r)", path.pushrev)
        revs = scmutil.revrange(repo, [expr])
        revs = [repo[rev].node() for rev in revs]
        if not revs:
            raise error.Abort(
                _("default push revset for path evaluates to an " "empty set")
            )

    opargs = dict(opts.get("opargs", {}))  # copy opargs since we may mutate it
    opargs.setdefault("pushvars", []).extend(opts.get("pushvars", []))

    pushop = exchange.push(
        repo,
        other,
        opts.get("force"),
        revs=revs,
        bookmarks=opts.get("bookmark", ()),
        opargs=opargs,
    )

    result = not pushop.cgresult

    if pushop.bkresult is not None:
        if pushop.bkresult == 2:
            result = 2
        elif not result and pushop.bkresult:
            result = 2

    return result


@command(
    "^record",
    [
        (
            "A",
            "addremove",
            None,
            _("mark new/missing files as added/removed before committing"),
        ),
        ("", "amend", None, _("amend the parent of the working directory")),
        ("s", "secret", None, _("use the secret phase for committing")),
        ("e", "edit", None, _("invoke editor on commit messages")),
    ]
    + commitopts
    + commitopts2
    + diffwsopts
    + walkopts,
    _("hg record [OPTION]... [FILE]..."),
)
def record(ui, repo, *pats, **opts):
    """interactively select changes to commit

    If a list of files is omitted, all changes reported by :hg:`status`
    will be candidates for recording.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    If using the text interface (see :hg:`help config`),
    you will be prompted for whether to record changes to each
    modified file, and for files with multiple changes, for each
    change to use. For each query, the following responses are
    possible::

      y - record this change
      n - skip this change
      e - edit this change manually

      s - skip remaining changes to this file
      f - record remaining changes to this file

      d - done, skip remaining changes and files
      a - record all changes to all remaining files
      q - quit, recording no changes

      ? - display help

    This command is not available when committing a merge."""

    if not ui.interactive():
        raise error.Abort(_("running non-interactively, use %s instead") % "commit")

    opts[r"interactive"] = True
    overrides = {("experimental", "crecord"): False}
    with ui.configoverride(overrides, "record"):
        return commit(ui, repo, *pats, **opts)


@command("recover", [])
def recover(ui, repo):
    """roll back an interrupted transaction

    Recover from an interrupted commit or pull.

    This command tries to fix the repository status after an
    interrupted operation. It should only be necessary when Mercurial
    suggests it.

    Returns 0 if successful, 1 if nothing to recover or verify fails.
    """
    if repo.recover():
        return hg.verify(repo, None)
    return 1


@command(
    "^remove|rm",
    [
        ("A", "after", None, _("record delete for missing files")),
        ("f", "force", None, _("forget added files, delete modified files")),
    ]
    + walkopts,
    _("[OPTION]... FILE..."),
    inferrepo=True,
)
def remove(ui, repo, *pats, **opts):
    """delete the specified tracked files

    Remove the specified tracked files from the repository and delete
    them. The files will be deleted from the repository at the next
    commit.

    To undo a remove before files have been committed, use :hg:`revert`.
    To stop tracking files without deleting them, use :hg:`forget`.

    .. container:: verbose

      -A/--after can be used to remove only files that have already
      been deleted, -f/--force can be used to force deletion, and -Af
      can be used to remove files from the next revision without
      deleting them from the working directory.

      The following table details the behavior of remove for different
      file states (columns) and option combinations (rows). The file
      states are Added [A], Clean [C], Modified [M] and Missing [!]
      (as reported by :hg:`status`). The actions are Warn, Remove
      (from branch) and Delete (from disk):

      ========= == == == ==
      opt/state A  C  M  !
      ========= == == == ==
      none      W  RD W  R
      -f        R  RD RD R
      -A        W  W  W  R
      -Af       R  R  R  R
      ========= == == == ==

      .. note::

         :hg:`remove` never deletes files in Added [A] state from the
         working directory, not even if ``--force`` is specified.

    Returns 0 on success, 1 if any warnings encountered.
    """

    opts = pycompat.byteskwargs(opts)
    after, force = opts.get("after"), opts.get("force")
    if not pats and not after:
        raise error.Abort(_("no files specified"))

    m = scmutil.match(repo[None], pats, opts)
    return cmdutil.remove(ui, repo, m, "", after, force)


@command(
    "rename|move|mv",
    [
        ("A", "after", None, _("record a rename that has already occurred")),
        ("f", "force", None, _("forcibly copy over an existing managed file")),
    ]
    + walkopts
    + dryrunopts,
    _("[OPTION]... SOURCE... DEST"),
)
def rename(ui, repo, *pats, **opts):
    """rename files; equivalent of copy + remove

    Mark dest as copies of sources; mark sources for deletion. If dest
    is a directory, copies are put in that directory. If dest is a
    file, there can only be one source.

    By default, this command copies the contents of files as they
    exist in the working directory. If invoked with -A/--after, the
    operation is recorded, but no copying is performed.

    This command takes effect at the next commit. To undo a rename
    before that, see :hg:`revert`.

    Returns 0 on success, 1 if errors are encountered.
    """
    opts = pycompat.byteskwargs(opts)
    with repo.wlock(False):
        return cmdutil.copy(ui, repo, pats, opts, rename=True)


@command(
    "resolve",
    [
        ("a", "all", None, _("select all unresolved files")),
        ("l", "list", None, _("list state of files needing merge")),
        ("m", "mark", None, _("mark files as resolved")),
        ("u", "unmark", None, _("mark files as unresolved")),
        ("n", "no-status", None, _("hide status prefix")),
        ("", "root-relative", None, _("show paths relative to repo root")),
    ]
    + mergetoolopts
    + walkopts
    + formatteropts,
    _("[OPTION]... [FILE]..."),
    inferrepo=True,
)
def resolve(ui, repo, *pats, **opts):
    """redo merges or set/view the merge status of files

    Merges with unresolved conflicts are often the result of
    non-interactive merging using the ``internal:merge`` configuration
    setting, or a command-line merge tool like ``diff3``. The resolve
    command is used to manage the files involved in a merge, after
    :hg:`merge` has been run, and before :hg:`commit` is run (i.e. the
    working directory must have two parents). See :hg:`help
    merge-tools` for information on configuring merge tools.

    The resolve command can be used in the following ways:

    - :hg:`resolve [--tool TOOL] FILE...`: attempt to re-merge the specified
      files, discarding any previous merge attempts. Re-merging is not
      performed for files already marked as resolved. Use ``--all/-a``
      to select all unresolved files. ``--tool`` can be used to specify
      the merge tool used for the given files. It overrides the HGMERGE
      environment variable and your configuration files.  Previous file
      contents are saved with a ``.orig`` suffix.

    - :hg:`resolve -m [FILE]`: mark a file as having been resolved
      (e.g. after having manually fixed-up the files). The default is
      to mark all unresolved files.

    - :hg:`resolve -u [FILE]...`: mark a file as unresolved. The
      default is to mark all resolved files.

    - :hg:`resolve -l`: list files which had or still have conflicts.
      In the printed list, ``U`` = unresolved and ``R`` = resolved.
      You can use ``set:unresolved()`` or ``set:resolved()`` to filter
      the list. See :hg:`help filesets` for details.

    .. note::

       Mercurial will not let you commit files with unresolved merge
       conflicts. You must use :hg:`resolve -m ...` before you can
       commit after a conflicting merge.

    Returns 0 on success, 1 if any files fail a resolve attempt.
    """

    opts = pycompat.byteskwargs(opts)
    flaglist = "all mark unmark list no_status root_relative".split()
    all, mark, unmark, show, nostatus, rootrel = [opts.get(o) for o in flaglist]

    # Enable --root-relative by default if HGPLAIN is set, for compatibility.
    if rootrel is None and ui.plain():
        rootrel = True

    if (show and (mark or unmark)) or (mark and unmark):
        raise error.Abort(_("too many options specified"))
    if pats and all:
        raise error.Abort(_("can't specify --all and patterns"))
    if not (all or pats or show or mark or unmark):
        raise error.Abort(
            _("no files or directories specified"),
            hint=("use --all to re-merge all unresolved files"),
        )

    if show:
        ui.pager("resolve")
        fm = ui.formatter("resolve", opts)
        ms = mergemod.mergestate.read(repo)
        m = scmutil.match(repo[None], pats, opts)

        # Labels and keys based on merge state.  Unresolved path conflicts show
        # as 'P'.  Resolved path conflicts show as 'R', the same as normal
        # resolved conflicts.
        mergestateinfo = {
            "u": ("resolve.unresolved", "U"),
            "r": ("resolve.resolved", "R"),
            "pu": ("resolve.unresolved", "P"),
            "pr": ("resolve.resolved", "R"),
            "d": ("resolve.driverresolved", "D"),
        }

        cwd = "" if rootrel else repo.getcwd()

        for f in ms:
            if not m(f):
                continue

            label, key = mergestateinfo[ms[f]]
            fm.startitem()
            fm.condwrite(not nostatus, "status", "%s ", key, label=label)
            # User-friendly paths
            f = repo.pathto(f, cwd)
            fm.write("path", "%s\n", f, label=label)
        fm.end()
        return 0

    with repo.wlock():
        ms = mergemod.mergestate.read(repo)

        if not (ms.active() or repo.dirstate.p2() != nullid):
            raise error.Abort(_("resolve command not applicable when not merging"))

        wctx = repo[None]

        if ms.mergedriver and ms.mdstate() == "u":
            proceed = mergemod.driverpreprocess(repo, ms, wctx)
            ms.commit()
            # allow mark and unmark to go through
            if not mark and not unmark and not proceed:
                return 1

        m = scmutil.match(wctx, pats, opts)
        ret = 0
        didwork = False
        runconclude = False

        tocomplete = []
        for f in ms:
            if not m(f):
                continue

            didwork = True

            # don't let driver-resolved files be marked, and run the conclude
            # step if asked to resolve
            if ms[f] == "d":
                exact = m.exact(f)
                if mark:
                    if exact:
                        ui.warn(_("not marking %s as it is driver-resolved\n") % f)
                elif unmark:
                    if exact:
                        ui.warn(_("not unmarking %s as it is driver-resolved\n") % f)
                else:
                    runconclude = True
                continue

            # path conflicts must be resolved manually
            if ms[f] in ("pu", "pr"):
                if mark:
                    ms.mark(f, "pr")
                elif unmark:
                    ms.mark(f, "pu")
                elif ms[f] == "pu":
                    ui.warn(_("%s: path conflict must be resolved manually\n") % f)
                continue

            if mark:
                ms.mark(f, "r")
            elif unmark:
                ms.mark(f, "u")
            else:
                # backup pre-resolve (merge uses .orig for its own purposes)
                a = repo.wjoin(f)
                try:
                    util.copyfile(a, a + ".resolve")
                except (IOError, OSError) as inst:
                    if inst.errno != errno.ENOENT:
                        raise

                try:
                    # preresolve file
                    ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "resolve")
                    complete, r = ms.preresolve(f, wctx)
                    if not complete:
                        tocomplete.append(f)
                    elif r:
                        ret = 1
                finally:
                    ui.setconfig("ui", "forcemerge", "", "resolve")
                    ms.commit()

                # replace filemerge's .orig file with our resolve file, but only
                # for merges that are complete
                if complete:
                    try:
                        util.rename(a + ".resolve", scmutil.origpath(ui, repo, a))
                    except OSError as inst:
                        if inst.errno != errno.ENOENT:
                            raise

        for f in tocomplete:
            try:
                # resolve file
                ui.setconfig("ui", "forcemerge", opts.get("tool", ""), "resolve")
                r = ms.resolve(f, wctx)
                if r:
                    ret = 1
            finally:
                ui.setconfig("ui", "forcemerge", "", "resolve")
                ms.commit()

            # replace filemerge's .orig file with our resolve file
            a = repo.wjoin(f)
            try:
                util.rename(a + ".resolve", scmutil.origpath(ui, repo, a))
            except OSError as inst:
                if inst.errno != errno.ENOENT:
                    raise

        ms.commit()
        ms.recordactions()

        if not didwork and pats:
            hint = None
            if not any([p for p in pats if p.find(":") >= 0]):
                pats = ["path:%s" % p for p in pats]
                m = scmutil.match(wctx, pats, opts)
                for f in ms:
                    if not m(f):
                        continue
                    flags = "".join(["-%s " % o[0] for o in flaglist if opts.get(o)])
                    hint = _("(try: hg resolve %s%s)\n") % (flags, " ".join(pats))
                    break
            ui.warn(_("arguments do not match paths that need resolving\n"))
            if hint:
                ui.warn(hint)
        elif ms.mergedriver and ms.mdstate() != "s":
            # run conclude step when either a driver-resolved file is requested
            # or there are no driver-resolved files
            # we can't use 'ret' to determine whether any files are unresolved
            # because we might not have tried to resolve some
            if (runconclude or not list(ms.driverresolved())) and not list(
                ms.unresolved()
            ):
                proceed = mergemod.driverconclude(repo, ms, wctx)
                ms.commit()
                if not proceed:
                    return 1

    # Nudge users into finishing an unfinished operation
    unresolvedf = list(ms.unresolved())
    driverresolvedf = list(ms.driverresolved())
    if not unresolvedf and not driverresolvedf:
        ui.status(_("(no more unresolved files)\n"))
        cmdutil.checkafterresolved(repo)
    elif not unresolvedf:
        ui.status(
            _("(no more unresolved files -- " 'run "hg resolve --all" to conclude)\n')
        )

    return ret


@command(
    "revert",
    [
        ("a", "all", None, _("revert all changes when no arguments given")),
        ("d", "date", "", _("tipmost revision matching date"), _("DATE")),
        ("r", "rev", "", _("revert to the specified revision"), _("REV")),
        ("C", "no-backup", None, _("do not save backup copies of files")),
        ("i", "interactive", None, _("interactively select the changes")),
    ]
    + walkopts
    + dryrunopts,
    _("[OPTION]... [-r REV] [NAME]..."),
)
def revert(ui, repo, *pats, **opts):
    """change the specified files to match a commit

    With no revision specified, restore the contents of files to an
    unmodified state and unschedule adds, removes, copies, and renames.
    In other words, revert the specified files or directories to the
    contents they had in the current commit. If you are in the middle of
    an unfinished merge state, you must explicitly specify a revision.

    Use the -r/--rev option to revert the given files or directories to
    their states as of a specific commit. Because revert does not actually
    check out the specified commit, the files appear as modified and show
    up as pending changes in :hg:`status`.

    Revert causes files to match their contents in another commit. If
    instead you want to undo a specific landed commit, use :hg:`backout`
    instead. Run `hg help backout` for more information.

    Modified files are saved with an .orig suffix before reverting.
    To disable these backups, use --no-backup. You can configure Mercurial
    to store these backup files in a custom directory relative to the root
    of the repository by setting the ``ui.origbackuppath`` configuration
    option.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    if opts.get("date"):
        if opts.get("rev"):
            raise error.Abort(_("you can't specify a revision and a date"))
        opts["rev"] = cmdutil.finddate(ui, repo, opts["date"])

    parent, p2 = repo.dirstate.parents()
    if not opts.get("rev") and p2 != nullid:
        # revert after merge is a trap for new users (issue2915)
        raise error.Abort(
            _("uncommitted merge with no revision specified"),
            hint=_("use 'hg update' or see 'hg help revert'"),
        )

    ctx = scmutil.revsingle(repo, opts.get("rev"))

    if not (
        pats
        or opts.get("include")
        or opts.get("exclude")
        or opts.get("all")
        or opts.get("interactive")
    ):
        msg = _("no files or directories specified")
        if p2 != nullid:
            hint = _(
                "uncommitted merge, use --all to discard all changes,"
                " or 'hg update -C .' to abort the merge"
            )
            raise error.Abort(msg, hint=hint)
        dirty = any(repo.status())
        node = ctx.node()
        if node != parent:
            if dirty:
                hint = (
                    _(
                        "uncommitted changes, use --all to discard all"
                        " changes, or 'hg update %s' to update"
                    )
                    % ctx.rev()
                )
            else:
                hint = (
                    _("use --all to revert all files," " or 'hg update %s' to update")
                    % ctx.rev()
                )
        elif dirty:
            hint = _("uncommitted changes, use --all to discard all changes")
        else:
            hint = _("use --all to revert all files")
        raise error.Abort(msg, hint=hint)

    return cmdutil.revert(
        ui, repo, ctx, (parent, p2), *pats, **pycompat.strkwargs(opts)
    )


@command("rollback", dryrunopts + [("f", "force", False, _("ignore safety measures"))])
def rollback(ui, repo, **opts):
    """roll back the last transaction (DANGEROUS) (DEPRECATED)

    Please use :hg:`commit --amend` instead of rollback to correct
    mistakes in the last commit.

    This command should be used with care. There is only one level of
    rollback, and there is no way to undo a rollback. It will also
    restore the dirstate at the time of the last transaction, losing
    any dirstate changes since that time. This command does not alter
    the working directory.

    Transactions are used to encapsulate the effects of all commands
    that create new changesets or propagate existing changesets into a
    repository.

    .. container:: verbose

      For example, the following commands are transactional, and their
      effects can be rolled back:

      - commit
      - import
      - pull
      - push (with this repository as the destination)
      - unbundle

      To avoid permanent data loss, rollback will refuse to rollback a
      commit transaction if it isn't checked out. Use --force to
      override this protection.

      The rollback command can be entirely disabled by setting the
      ``ui.rollback`` configuration setting to false. If you're here
      because you want to use rollback and it's disabled, you can
      re-enable the command by setting ``ui.rollback`` to true.

    This command is not intended for use on public repositories. Once
    changes are visible for pull by other users, rolling a transaction
    back locally is ineffective (someone else may already have pulled
    the changes). Furthermore, a race is possible with readers of the
    repository; for example an in-progress pull from the repository
    may fail if a rollback is performed.

    Returns 0 on success, 1 if no rollback data is available.
    """
    if not ui.configbool("ui", "rollback"):
        raise error.Abort(
            _("rollback is disabled because it is unsafe"),
            hint=("see `hg help -v rollback` for information"),
        )
    return repo.rollback(dryrun=opts.get(r"dry_run"), force=opts.get(r"force"))


@command("root", [("", "shared", None, _("show root of the shared repo"))])
def root(ui, repo, **opts):
    """print the root (top) of the current working directory

    Print the root directory of the current repository.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)

    if opts.get("shared"):
        # remove the tailing /.hg from output
        sharedpath = os.path.dirname(repo.sharedpath)
        ui.write(sharedpath + "\n")
    else:
        ui.write(repo.root + "\n")


@command(
    "^serve",
    [
        ("A", "accesslog", "", _("name of access log file to write to"), _("FILE")),
        ("d", "daemon", None, _("run server in background")),
        ("", "daemon-postexec", [], _("used internally by daemon mode")),
        ("E", "errorlog", "", _("name of error log file to write to"), _("FILE")),
        # use string type, then we can check if something was passed
        ("p", "port", "", _("port to listen on (default: 8000)"), _("PORT")),
        (
            "a",
            "address",
            "",
            _("address to listen on (default: all interfaces)"),
            _("ADDR"),
        ),
        (
            "",
            "prefix",
            "",
            _("prefix path to serve from (default: server root)"),
            _("PREFIX"),
        ),
        (
            "n",
            "name",
            "",
            _("name to show in web pages (default: working directory)"),
            _("NAME"),
        ),
        (
            "",
            "web-conf",
            "",
            _("name of the hgweb config file (see 'hg help hgweb')"),
            _("FILE"),
        ),
        (
            "",
            "webdir-conf",
            "",
            _("name of the hgweb config file (DEPRECATED)"),
            _("FILE"),
        ),
        ("", "pid-file", "", _("name of file to write process ID to"), _("FILE")),
        (
            "",
            "port-file",
            "",
            _("name of file to write port to (useful with '-p 0')"),
            _("FILE"),
        ),
        ("", "stdio", None, _("for remote clients (ADVANCED)")),
        ("", "cmdserver", "", _("for remote clients (ADVANCED)"), _("MODE")),
        ("t", "templates", "", _("web templates to use"), _("TEMPLATE")),
        ("", "style", "", _("template style to use"), _("STYLE")),
        ("6", "ipv6", None, _("use IPv6 in addition to IPv4")),
        ("", "certificate", "", _("SSL certificate file"), _("FILE")),
        ("", "read-only", None, _("only allow read operations")),
    ],
    _("[OPTION]..."),
    optionalrepo=True,
)
def serve(ui, repo, **opts):
    """start stand-alone webserver

    Start a local HTTP repository browser and pull server. You can use
    this for ad-hoc sharing and browsing of repositories. It is
    recommended to use a real web server to serve a repository for
    longer periods of time.

    Please note that the server does not implement access control.
    This means that, by default, anybody can read from the server and
    nobody can write to it by default. Set the ``web.allow-push``
    option to ``*`` to allow everybody to push to the server. You
    should use a real web server if you need to authenticate users.

    By default, the server logs accesses to stdout and errors to
    stderr. Use the -A/--accesslog and -E/--errorlog options to log to
    files.

    To have the server choose a free port number to listen on, specify
    a port number of 0; in this case, the server will print the port
    number it uses.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    if opts["stdio"] and opts["cmdserver"]:
        raise error.Abort(_("cannot use --stdio with --cmdserver"))

    if opts["stdio"]:
        if repo is None:
            raise error.RepoError(
                _("there is no Mercurial repository here" " (.hg not found)")
            )
        s = sshserver.sshserver(ui, repo)
        s.serve_forever()

    if opts.get("cmdserver") in ["chgunix", "chgunix2"]:
        raise error.ProgrammingError(
            "chgunix server cannot be started via traditional command code path"
        )

    service = server.createservice(ui, repo, opts)
    return server.runservice(opts, initfn=service.init, runfn=service.run)


@command(
    "^show",
    [
        (
            "",
            "nodates",
            None,
            _("omit dates from diff headers (but keeps it in commit header)"),
        ),
        ("", "noprefix", None, _("omit a/ and b/ prefixes from filenames")),
        ("", "stat", None, _("output diffstat-style summary of changes")),
        ("g", "git", None, _("use git extended diff format")),
        ("U", "unified", int, _("number of lines of diff context to show")),
    ]
    + diffwsopts
    + templateopts
    + walkopts,
    _("[OPTION]... [REV [FILE]...]"),
    inferrepo=True,
    cmdtype=readonly,
)
def show(ui, repo, *args, **opts):
    """show commit in detail

    Show the commit message and contents for the specified commit. If no commit
    is specified, shows the current commit.

    This behaves similarly to :hg:`log -vp -r REV [OPTION]... [FILE]...`, or
    if called without a REV, :hg:`log -vp -r . [OPTION]...` Use
    :hg:`log` for more powerful operations than supported by hg show.

    """
    ui.pager("show")
    if len(args) == 0:
        opts["rev"] = ["."]
        pats = []
    else:
        opts["rev"] = [args[0]]
        pats = args[1:]
        if not scmutil.revrange(repo, opts["rev"]):
            h = _("if %s is a file, try `hg show . %s`") % (args[0], args[0])
            raise error.Abort(_("unknown revision %s") % args[0], hint=h)

    opts["patch"] = not opts["stat"]
    opts["verbose"] = True

    # Copy tracking is slow when doing a git diff. Override hgrc, and rely on
    # opts getting us a git diff if it's been requested. Ideally, we'd find and
    # fix the slowness in copy tracking, but this works for now.
    # On a commit with lots of possible copies, Bryan O'Sullivan found that this
    # reduces "time hg show" from 1.76 seconds to 0.81 seconds.
    overrides = {
        ("diff", "git"): opts.get("git"),
        ("diff", "unified"): opts.get("unified"),
        ("ui", "verbose"): True,
    }

    logcmd, defaultlogopts = cmdutil.getcmdanddefaultopts("log", table)
    defaultlogopts.update(opts)

    with ui.configoverride(overrides, "show"):
        logcmd(ui, repo, *pats, **defaultlogopts)


@command(
    "^status|st",
    [
        ("A", "all", None, _("show status of all files")),
        ("m", "modified", None, _("show only modified files")),
        ("a", "added", None, _("show only added files")),
        ("r", "removed", None, _("show only removed files")),
        ("d", "deleted", None, _("show only deleted (but tracked) files")),
        ("c", "clean", None, _("show only files without changes")),
        ("u", "unknown", None, _("show only unknown (not tracked) files")),
        ("i", "ignored", None, _("show only ignored files")),
        ("n", "no-status", None, _("hide status prefix")),
        ("t", "terse", "", _("show the terse output (EXPERIMENTAL)")),
        ("C", "copies", None, _("show source of copied files")),
        ("0", "print0", None, _("end filenames with NUL, for use with xargs")),
        ("", "rev", [], _("show difference from revision"), _("REV")),
        ("", "change", "", _("list the changed files of a revision"), _("REV")),
    ]
    + walkopts
    + formatteropts,
    _("[OPTION]... [FILE]..."),
    inferrepo=True,
    cmdtype=readonly,
)
def status(ui, repo, *pats, **opts):
    """list files with pending changes

    Show status of files in the repository using the following status
    indicators::

      M = modified
      A = added
      R = removed
      C = clean
      ! = missing (deleted by a non-hg command, but still tracked)
      ? = not tracked
      I = ignored
        = origin of the previous file (with --copies)

    By default, shows files that have been modified, added, removed,
    deleted, or that are unknown (corresponding to the options -mardu).
    Files that are unmodified, ignored, or the source of a copy/move
    operation are not listed.

    To control the exact statuses that are shown, specify the relevant
    flags (like -rd to show only files that are removed or deleted).
    Additionally, specify -q/--quiet to hide both unknown and ignored
    files.

    To show the status of specific files, provide an explicit list of
    files to match. To include or exclude files using regular expressions,
    use -I or -X.

    If --rev is specified, and only one revision is given, it is used as
    the base revision. If two revisions are given, the differences between
    them are shown. The --change option can also be used as a shortcut
    to list the changed files of a revision from its first parent.

    .. note::

       :hg:`status` might appear to disagree with :hg:`diff` if permissions
       have changed or a merge has occurred, because the standard diff
       format does not report permission changes and :hg:`diff` only
       reports changes relative to one merge parent.

    .. container:: verbose

      The -t/--terse option abbreviates the output by showing only the directory
      name if all the files in it share the same status. The option takes an
      argument indicating the statuses to abbreviate: 'm' for 'modified', 'a'
      for 'added', 'r' for 'removed', 'd' for 'deleted', 'u' for 'unknown', 'i'
      for 'ignored' and 'c' for clean.

      It abbreviates only those statuses which are passed. Note that clean and
      ignored files are not displayed with '--terse ic' unless the -c/--clean
      and -i/--ignored options are also used.

      The -v/--verbose option shows information when the repository is in an
      unfinished merge, shelve, rebase state etc. You can have this behavior
      turned on by default by enabling the ``commands.status.verbose`` option.

      You can skip displaying some of these states by setting
      ``commands.status.skipstates`` to one or more of: 'bisect', 'graft',
      'histedit', 'merge', 'rebase', or 'unshelve'.

      Examples:

      - show changes in the working directory relative to a
        changeset::

          hg status --rev 9353

      - show changes in the working directory relative to the
        current directory (see :hg:`help patterns` for more information)::

          hg status re:

      - show all changes including copies in an existing changeset::

          hg status --copies --change 9353

      - get a NUL separated list of added files, suitable for xargs::

          hg status -an0

      - show more information about the repository status, abbreviating
        added, removed, modified, deleted, and untracked paths::

          hg status -v -t mardu

    Returns 0 on success.

    """

    opts = pycompat.byteskwargs(opts)
    revs = opts.get("rev")
    change = opts.get("change")
    terse = opts.get("terse")

    if revs and change:
        msg = _("cannot specify --rev and --change at the same time")
        raise error.Abort(msg)
    elif revs and terse:
        msg = _("cannot use --terse with --rev")
        raise error.Abort(msg)
    elif change:
        node2 = scmutil.revsingle(repo, change, None).node()
        node1 = repo[node2].p1().node()
    else:
        node1, node2 = scmutil.revpair(repo, revs)

    if pats or ui.configbool("commands", "status.relative"):
        cwd = repo.getcwd()
    else:
        cwd = ""

    if opts.get("print0"):
        end = "\0"
    else:
        end = "\n"
    copy = {}
    states = "modified added removed deleted unknown ignored clean".split()
    show = [k for k in states if opts.get(k)]
    if opts.get("all"):
        show += ui.quiet and (states[:4] + ["clean"]) or states

    if not show:
        if ui.quiet:
            show = states[:4]
        else:
            show = states[:5]

    m = scmutil.match(repo[node2], pats, opts)
    if terse:
        # we need to compute clean and unknown to terse
        stat = repo.status(
            node1, node2, m, "ignored" in show or "i" in terse, True, True
        )

        stat = cmdutil.tersedir(stat, terse)
    else:
        stat = repo.status(
            node1, node2, m, "ignored" in show, "clean" in show, "unknown" in show
        )

    changestates = zip(states, pycompat.iterbytestr("MAR!?IC"), stat)

    if (
        opts.get("all") or opts.get("copies") or ui.configbool("ui", "statuscopies")
    ) and not opts.get("no_status"):
        copy = copies.pathcopies(repo[node1], repo[node2], m)

    ui.pager("status")
    fm = ui.formatter("status", opts)
    fmt = "%s" + end
    showchar = not opts.get("no_status")

    for state, char, files in changestates:
        if state in show:
            label = "status." + state
            for f in files:
                fm.startitem()
                fm.condwrite(showchar, "status", "%s ", char, label=label)
                fm.write("path", fmt, repo.pathto(f, cwd), label=label)
                if f in copy:
                    fm.write(
                        "copy",
                        "  %s" + end,
                        repo.pathto(copy[f], cwd),
                        label="status.copied",
                    )

    if (ui.verbose or ui.configbool("commands", "status.verbose")) and not ui.plain():
        cmdutil.morestatus(repo, fm)
    fm.end()


@command(
    "^summary|sum",
    [("", "remote", None, _("check for push and pull"))],
    "[--remote]",
    cmdtype=readonly,
)
def summary(ui, repo, **opts):
    """summarize working directory state

    This generates a brief summary of the working directory state,
    including parents, branch, commit status, phase and available updates.

    With the --remote option, this will check the default paths for
    incoming and outgoing changes. This can be time-consuming.

    Returns 0 on success.
    """

    opts = pycompat.byteskwargs(opts)
    ui.pager("summary")
    ctx = repo[None]
    parents = ctx.parents()
    marks = []

    ms = None
    try:
        ms = mergemod.mergestate.read(repo)
    except error.UnsupportedMergeRecords as e:
        s = " ".join(e.recordtypes)
        ui.warn(_("warning: merge state has unsupported record types: %s\n") % s)
        unresolved = []
    else:
        unresolved = list(ms.unresolved())

    for p in parents:
        # label with log.changeset (instead of log.parent) since this
        # shows a working directory parent *changeset*:
        # i18n: column positioning for "hg summary"
        ui.write(_("parent: %d:%s ") % (p.rev(), p), label=cmdutil._changesetlabels(p))
        ui.write(" ".join(p.tags()), label="log.tag")
        if p.bookmarks():
            marks.extend(p.bookmarks())
        if p.rev() == -1:
            if not len(repo):
                ui.write(_(" (empty repository)"))
            else:
                ui.write(_(" (no revision checked out)"))
        if p.obsolete():
            ui.write(_(" (obsolete)"))
        if p.isunstable():
            instabilities = (
                ui.label(instability, "trouble.%s" % instability)
                for instability in p.instabilities()
            )
            ui.write(" (" + ", ".join(instabilities) + ")")
        ui.write("\n")
        if p.description():
            ui.status(
                " " + p.description().splitlines()[0].strip() + "\n",
                label="log.summary",
            )

    if marks:
        active = repo._activebookmark
        # i18n: column positioning for "hg summary"
        ui.write(_("bookmarks:"), label="log.bookmark")
        if active is not None:
            if active in marks:
                ui.write(" *" + active, label=bookmarks.activebookmarklabel)
                marks.remove(active)
            else:
                ui.write(" [%s]" % active, label=bookmarks.activebookmarklabel)
        for m in marks:
            ui.write(" " + m, label="log.bookmark")
        ui.write("\n", label="log.bookmark")

    status = repo.status(unknown=True)

    c = repo.dirstate.copies()
    copied, renamed = [], []
    for d, s in c.iteritems():
        if s in status.removed:
            status.removed.remove(s)
            renamed.append(d)
        else:
            copied.append(d)
        if d in status.added:
            status.added.remove(d)

    labels = [
        (ui.label(_("%d modified"), "status.modified"), status.modified),
        (ui.label(_("%d added"), "status.added"), status.added),
        (ui.label(_("%d removed"), "status.removed"), status.removed),
        (ui.label(_("%d renamed"), "status.copied"), renamed),
        (ui.label(_("%d copied"), "status.copied"), copied),
        (ui.label(_("%d deleted"), "status.deleted"), status.deleted),
        (ui.label(_("%d unknown"), "status.unknown"), status.unknown),
        (ui.label(_("%d unresolved"), "resolve.unresolved"), unresolved),
    ]
    t = []
    for l, s in labels:
        if s:
            t.append(l % len(s))

    t = ", ".join(t)
    cleanworkdir = False

    if repo.localvfs.exists("graftstate"):
        t += _(" (graft in progress)")
    if repo.localvfs.exists("updatestate"):
        t += _(" (interrupted update)")
    elif len(parents) > 1:
        t += _(" (merge)")
    elif not (status.modified or status.added or status.removed or renamed or copied):
        t += _(" (clean)")
        cleanworkdir = True

    if parents:
        pendingphase = max(p.phase() for p in parents)
    else:
        pendingphase = phases.public

    if pendingphase > phases.newcommitphase(ui):
        t += " (%s)" % phases.phasenames[pendingphase]

    if cleanworkdir:
        # i18n: column positioning for "hg summary"
        ui.status(_("commit: %s\n") % t.strip())
    else:
        # i18n: column positioning for "hg summary"
        ui.write(_("commit: %s\n") % t.strip())

    t = []
    draft = len(repo.revs("draft()"))
    if draft:
        t.append(_("%d draft") % draft)
    secret = len(repo.revs("secret()"))
    if secret:
        t.append(_("%d secret") % secret)

    if draft or secret:
        ui.status(_("phases: %s\n") % ", ".join(t))

    if obsolete.isenabled(repo, obsolete.createmarkersopt) and not mutation.enabled(
        repo
    ):
        for trouble in ("orphan", "contentdivergent", "phasedivergent"):
            numtrouble = len(repo.revs(trouble + "()"))
            # We write all the possibilities to ease translation
            troublemsg = {
                "orphan": _("orphan: %d changesets"),
                "contentdivergent": _("content-divergent: %d changesets"),
                "phasedivergent": _("phase-divergent: %d changesets"),
            }
            if numtrouble > 0:
                ui.status(troublemsg[trouble] % numtrouble + "\n")

    cmdutil.summaryhooks(ui, repo)

    if opts.get("remote"):
        needsincoming, needsoutgoing = True, True
    else:
        needsincoming, needsoutgoing = False, False
        for i, o in cmdutil.summaryremotehooks(ui, repo, opts, None):
            if i:
                needsincoming = True
            if o:
                needsoutgoing = True
        if not needsincoming and not needsoutgoing:
            return

    def getincoming():
        source, branches = hg.parseurl(ui.expandpath("default"))
        sbranch = branches[0]
        try:
            other = hg.peer(repo, {}, source)
        except error.RepoError:
            if opts.get("remote"):
                raise
            return source, sbranch, None, None, None
        revs, checkout = hg.addbranchrevs(repo, other, branches, None)
        if revs:
            revs = [other.lookup(rev) for rev in revs]
        ui.debug("comparing with %s\n" % util.hidepassword(source))
        repo.ui.pushbuffer()
        commoninc = discovery.findcommonincoming(repo, other, heads=revs)
        repo.ui.popbuffer()
        return source, sbranch, other, commoninc, commoninc[1]

    if needsincoming:
        source, sbranch, sother, commoninc, incoming = getincoming()
    else:
        source = sbranch = sother = commoninc = incoming = None

    def getoutgoing():
        dest, branches = hg.parseurl(ui.expandpath("default-push", "default"))
        dbranch = branches[0]
        revs, checkout = hg.addbranchrevs(repo, repo, branches, None)
        if source != dest:
            try:
                dother = hg.peer(repo, {}, dest)
            except error.RepoError:
                if opts.get("remote"):
                    raise
                return dest, dbranch, None, None
            ui.debug("comparing with %s\n" % util.hidepassword(dest))
        elif sother is None:
            # there is no explicit destination peer, but source one is invalid
            return dest, dbranch, None, None
        else:
            dother = sother
        if source != dest or (sbranch is not None and sbranch != dbranch):
            common = None
        else:
            common = commoninc
        if revs:
            revs = [repo.lookup(rev) for rev in revs]
        repo.ui.pushbuffer()
        outgoing = discovery.findcommonoutgoing(
            repo, dother, onlyheads=revs, commoninc=common
        )
        repo.ui.popbuffer()
        return dest, dbranch, dother, outgoing

    if needsoutgoing:
        dest, dbranch, dother, outgoing = getoutgoing()
    else:
        dest = dbranch = dother = outgoing = None

    if opts.get("remote"):
        t = []
        if incoming:
            t.append(_("1 or more incoming"))
        o = outgoing.missing
        if o:
            t.append(_("%d outgoing") % len(o))
        other = dother or sother
        if "bookmarks" in other.listkeys("namespaces"):
            counts = bookmarks.summary(repo, other)
            if counts[0] > 0:
                t.append(_("%d incoming bookmarks") % counts[0])
            if counts[1] > 0:
                t.append(_("%d outgoing bookmarks") % counts[1])

        if t:
            # i18n: column positioning for "hg summary"
            ui.write(_("remote: %s\n") % (", ".join(t)))
        else:
            # i18n: column positioning for "hg summary"
            ui.status(_("remote: (synced)\n"))

    cmdutil.summaryremotehooks(
        ui,
        repo,
        opts,
        ((source, sbranch, sother, commoninc), (dest, dbranch, dother, outgoing)),
    )


@command(
    "tag",
    [
        ("f", "force", None, _("force tag")),
        ("l", "local", None, _("make the tag local")),
        ("r", "rev", "", _("revision to tag"), _("REV")),
        ("", "remove", None, _("remove a tag")),
        # -l/--local is already there, commitopts cannot be used
        ("e", "edit", None, _("invoke editor on commit messages")),
        ("m", "message", "", _("use text as commit message"), _("TEXT")),
    ]
    + commitopts2,
    _("[-f] [-l] [-m TEXT] [-d DATE] [-u USER] [-r REV] NAME..."),
)
def tag(ui, repo, name1, *names, **opts):
    """add one or more tags for the current or given revision

    Name a particular revision using <name>.

    Tags are used to name particular revisions of the repository and are
    very useful to compare different revisions, to go back to significant
    earlier versions or to mark branch points as releases, etc. Changing
    an existing tag is normally disallowed; use -f/--force to override.

    If no revision is given, the parent of the working directory is
    used.

    To facilitate version control, distribution, and merging of tags,
    they are stored as a file named ".hgtags" which is managed similarly
    to other project files and can be hand-edited if necessary. This
    also means that tagging creates a new commit. The file
    ".hg/localtags" is used for local tags (not shared among
    repositories).

    Tag commits are usually made at the head of a branch. If the parent
    of the working directory is not a branch head, :hg:`tag` aborts; use
    -f/--force to force the tag commit to be based on a non-head
    changeset.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Since tag names have priority over branch names during revision
    lookup, using an existing branch name as a tag name is discouraged.

    .. container:: verbose

      The tag commands can be entirely disabled by setting the
      ``ui.allowtags`` configuration setting to false.

    Returns 0 on success.
    """
    if not ui.configbool("ui", "allowtags", default=True):
        raise error.Abort(_("new tags are disabled in this repository"))
    opts = pycompat.byteskwargs(opts)
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        rev_ = "."
        names = [t.strip() for t in (name1,) + names]
        if len(names) != len(set(names)):
            raise error.Abort(_("tag names must be unique"))
        for n in names:
            scmutil.checknewlabel(repo, n, "tag")
            if not n:
                raise error.Abort(
                    _("tag names cannot consist entirely of " "whitespace")
                )
        if opts.get("rev") and opts.get("remove"):
            raise error.Abort(_("--rev and --remove are incompatible"))
        if opts.get("rev"):
            rev_ = opts["rev"]
        message = opts.get("message")
        if opts.get("remove"):
            if opts.get("local"):
                expectedtype = "local"
            else:
                expectedtype = "global"

            for n in names:
                if not repo.tagtype(n):
                    raise error.Abort(_("tag '%s' does not exist") % n)
                if repo.tagtype(n) != expectedtype:
                    if expectedtype == "global":
                        raise error.Abort(_("tag '%s' is not a global tag") % n)
                    else:
                        raise error.Abort(_("tag '%s' is not a local tag") % n)
            rev_ = "null"
            if not message:
                # we don't translate commit messages
                message = "Removed tag %s" % ", ".join(names)
        elif not opts.get("force"):
            for n in names:
                if n in repo.tags():
                    raise error.Abort(
                        _("tag '%s' already exists " "(use -f to force)") % n
                    )
        if not opts.get("local"):
            p1, p2 = repo.dirstate.parents()
            if p2 != nullid:
                raise error.Abort(_("uncommitted merge"))
            bheads = repo.branchheads()
            if not opts.get("force") and bheads and p1 not in bheads:
                raise error.Abort(
                    _("working directory is not at a branch head " "(use -f to force)")
                )
        r = scmutil.revsingle(repo, rev_).node()

        if not message:
            # we don't translate commit messages
            message = "Added tag %s for changeset %s" % (", ".join(names), short(r))

        date = opts.get("date")
        if date:
            date = util.parsedate(date)

        if opts.get("remove"):
            editform = "tag.remove"
        else:
            editform = "tag.add"
        editor = cmdutil.getcommiteditor(editform=editform, **pycompat.strkwargs(opts))

        # don't allow tagging the null rev
        if not opts.get("remove") and scmutil.revsingle(repo, rev_).rev() == nullrev:
            raise error.Abort(_("cannot tag null revision"))

        tagsmod.tag(
            repo,
            names,
            r,
            message,
            opts.get("local"),
            opts.get("user"),
            date,
            editor=editor,
        )
    finally:
        release(lock, wlock)


@command("tags", formatteropts, "", cmdtype=readonly)
def tags(ui, repo, **opts):
    """list repository tags

    This lists both regular and local tags. When the -v/--verbose
    switch is used, a third column "local" is printed for local tags.
    When the -q/--quiet switch is used, only the tag name is printed.

    Returns 0 on success.
    """
    if not ui.configbool("ui", "allowtags", default=True):
        raise error.Abort(_("tags are disabled in this repository"))
    opts = pycompat.byteskwargs(opts)
    ui.pager("tags")
    fm = ui.formatter("tags", opts)
    hexfunc = fm.hexfunc
    tagtype = ""

    for t, n in reversed(repo.tagslist()):
        hn = hexfunc(n)
        label = "tags.normal"
        tagtype = ""
        if repo.tagtype(t) == "local":
            label = "tags.local"
            tagtype = "local"

        fm.startitem()
        fm.write("tag", "%s", t, label=label)
        fmt = " " * (30 - encoding.colwidth(t)) + " %5d:%s"
        fm.condwrite(
            not ui.quiet, "rev node", fmt, repo.changelog.rev(n), hn, label=label
        )
        fm.condwrite(ui.verbose and tagtype, "type", " %s", tagtype, label=label)
        fm.plain("\n")
    fm.end()


@command(
    "tip",
    [
        ("p", "patch", None, _("show patch")),
        ("g", "git", None, _("use git extended diff format")),
    ]
    + templateopts,
    _("[-p] [-g]"),
)
def tip(ui, repo, **opts):
    """show the tip revision (DEPRECATED)

    The tip revision (usually just called the tip) is the changeset
    most recently added to the repository (and therefore the most
    recently changed head).

    If you have just made a commit, that commit will be the tip. If
    you have just pulled changes from another repository, the tip of
    that repository becomes the current tip. The "tip" tag is special
    and cannot be renamed or assigned to a different changeset.

    This command is deprecated, please use :hg:`heads` instead.

    Returns 0 on success.
    """
    opts = pycompat.byteskwargs(opts)
    displayer = cmdutil.show_changeset(ui, repo, opts)
    displayer.show(repo["tip"])
    displayer.close()


@command(
    "unbundle",
    [
        (
            "u",
            "update",
            None,
            _("update to new branch head if changesets were unbundled"),
        )
    ],
    _("[-u] FILE..."),
)
def unbundle(ui, repo, fname1, *fnames, **opts):
    """apply one or more bundle files

    Apply one or more bundle files generated by :hg:`bundle`.

    Returns 0 on success, 1 if an update has unresolved files.
    """
    fnames = (fname1,) + fnames

    with repo.lock():
        for fname in fnames:
            f = hg.openpath(ui, fname)
            gen = exchange.readbundle(ui, f, fname)
            if isinstance(gen, streamclone.streamcloneapplier):
                raise error.Abort(
                    _("packed bundles cannot be applied with " '"hg unbundle"'),
                    hint=_('use "hg debugapplystreamclonebundle"'),
                )
            url = "bundle:" + fname
            try:
                txnname = "unbundle"
                if not isinstance(gen, bundle2.unbundle20):
                    txnname = "unbundle\n%s" % util.hidepassword(url)
                with repo.transaction(txnname) as tr:
                    op = bundle2.applybundle(repo, gen, tr, source="unbundle", url=url)
            except error.BundleUnknownFeatureError as exc:
                raise error.Abort(
                    _("%s: unknown bundle feature, %s") % (fname, exc),
                    hint=_(
                        "see https://mercurial-scm.org/"
                        "wiki/BundleFeature for more "
                        "information"
                    ),
                )
            modheads = bundle2.combinechangegroupresults(op)

    return postincoming(ui, repo, modheads, opts.get(r"update"), None, None)


@command(
    "^update|up|checkout|co",
    [
        ("C", "clean", None, _("discard uncommitted changes (no backup)")),
        ("c", "check", None, _("require clean working directory")),
        ("m", "merge", None, _("merge uncommitted changes")),
        ("d", "date", "", _("tipmost revision matching date (ADVANCED)"), _("DATE")),
        ("r", "rev", "", _("revision"), _("REV")),
        ("", "inactive", None, _("update without activating bookmarks")),
    ]
    + mergetoolopts,
    _("[-C|-c|-m] [[-r] REV]"),
)
def update(
    ui,
    repo,
    node=None,
    rev=None,
    clean=False,
    date=None,
    check=False,
    merge=None,
    tool=None,
    inactive=None,
):
    """check out a specific commit

    Update your checkout to the given destination commit. More precisely, make
    the destination commit the current commit and update the contents of all
    files in your checkout to match their state in the destination commit.

    By default, if you attempt to check out a commit while you have pending
    changes, and the destination commit is not an ancestor or descendant of
    the current commit, the checkout will abort. However, if the destination
    commit is an ancestor or descendant of the current commit, the pending
    changes will be merged into the new checkout.

    Use one of the following flags to modify this behavior:

    --check: abort if there are pending changes

    --clean: permanently discard any pending changes (use with caution)

    --merge: attempt to merge the pending changes into the new checkout, even
    if the destination commit is not an ancestor or descendant of the current
    commit

    If merge conflicts occur during checkout, Mercurial enters an unfinished
    merge state. If this happens, fix the conflicts manually and then run
    hg commit to exit the unfinished merge state and save your changes in a
    new commit. Alternatively, run hg checkout --clean to discard your pending
    changes.

    Specify null as the destination commit to get an empty checkout (sometimes
    known as a bare repository).

    Returns 0 on success, 1 if there are unresolved files.
    """
    if rev is not None and rev != "" and node is not None:
        raise error.Abort(_("please specify just one revision"))

    if ui.configbool("commands", "update.requiredest"):
        if not node and not rev and not date:
            raise error.Abort(
                _("you must specify a destination"),
                hint=_('for example: hg update ".::"'),
            )

    if rev is None or rev == "":
        rev = node

    if date and rev is not None:
        raise error.Abort(_("you can't specify a revision and a date"))

    if len([x for x in (clean, check, merge) if x]) > 1:
        raise error.Abort(
            _("can only specify one of -C/--clean, -c/--check, " "or -m/--merge")
        )

    # If nothing was passed, the default behavior (moving to branch tip)
    # can be disabled and replaced with an error.
    # internal config: ui.disallowemptyupdate
    if ui.configbool("ui", "disallowemptyupdate"):
        if node is None and rev is None and not date:
            raise error.Abort(
                "You must specify a destination to update to,"
                + ' for example "hg update master".',
                hint="If you're trying to move a bookmark forward, try "
                + '"hg rebase -d <destination>".',
            )

    # Suggest `hg prev` as an alternative to 'hg update .^'.
    # internal config: ui.suggesthgprev
    if node == ".^" and ui.configbool("ui", "suggesthgprev", False):
        hintutil.trigger("update-prev")

    updatecheck = None
    if check:
        updatecheck = "abort"
    elif merge:
        updatecheck = "none"

    with repo.wlock():
        cmdutil.clearunfinished(repo)

        if date:
            rev = cmdutil.finddate(ui, repo, date)

        if inactive:
            brev = None
        else:
            brev = rev
        rev = scmutil.revsingle(repo, rev, rev).rev()

        repo.ui.setconfig("ui", "forcemerge", tool, "update")

        return hg.updatetotally(
            ui, repo, rev, brev, clean=clean, updatecheck=updatecheck
        )


@command(
    "verify", [("r", "rev", [], _("verify the specified revision or revset"), _("REV"))]
)
def verify(ui, repo, **opts):
    """verify the integrity of the repository

    Verify the integrity of the current repository.

    This will perform an extensive check of the repository's
    integrity, validating the hashes and checksums of each entry in
    the changelog, manifest, and tracked files, as well as the
    integrity of their crosslinks and indices.

    Please see https://mercurial-scm.org/wiki/RepositoryCorruption
    for more information about recovery from corruption of the
    repository.

    Returns 0 on success, 1 if errors are encountered.

    Manifest verification can be extremely slow on large repos, so it can be
    disabled if ``verify.skipmanifests`` is True::

        [verify]
            skipmanifests = true
    """
    revrange = opts.get("rev")
    if revrange:
        revs = scmutil.revrange(repo, revrange)
    else:
        revs = None

    return hg.verify(repo, revs)


@command("version", [] + formatteropts, norepo=True, cmdtype=readonly)
def version_(ui, **opts):
    """output version and copyright information"""
    opts = pycompat.byteskwargs(opts)
    if ui.verbose:
        ui.pager("version")
    fm = ui.formatter("version", opts)
    fm.startitem()
    fm.write("ver", _("@LongProduct@ (version %s)\n"), util.version())
    license = _(
        "(see https://mercurial-scm.org for more information)\n"
        "\nCopyright (C) 2005-2017 Matt Mackall and others\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    )
    if not ui.quiet:
        fm.plain(license)

    if ui.verbose:
        fm.plain(_("\nEnabled extensions:\n\n"))
    # format names and versions into columns
    names = []
    vers = []
    isinternals = []
    for name, module in extensions.extensions():
        names.append(name)
        vers.append(extensions.moduleversion(module) or None)
        isinternals.append(extensions.ismoduleinternal(module))
    fn = fm.nested("extensions")
    if names:
        namefmt = "  %%-%ds  " % max(len(n) for n in names)
        places = [_("external"), _("internal")]
        for n, v, p in zip(names, vers, isinternals):
            fn.startitem()
            fn.condwrite(ui.verbose, "name", namefmt, n)
            if ui.verbose:
                fn.plain("%s  " % places[p])
            fn.data(bundled=p)
            fn.condwrite(ui.verbose and v, "ver", "%s", v)
            if ui.verbose:
                fn.plain("\n")
    fn.end()
    fm.end()


def loadcmdtable(ui, name, cmdtable):
    """Load command functions from specified cmdtable
    """
    overrides = [cmd for cmd in cmdtable if cmd in table]
    if overrides:
        ui.warn(
            _("extension '%s' overrides commands: %s\n") % (name, " ".join(overrides))
        )
    table.update(cmdtable)

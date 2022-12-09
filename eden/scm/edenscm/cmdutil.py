# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# cmdutil.py - help for command processing in mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import itertools
import os
import re
import stat
import tempfile
from typing import Dict

import bindings
from bindings import renderdag
from edenscm import tracing

from . import (
    bookmarks,
    changelog,
    copies,
    crecord as crecordmod,
    dagop,
    dirstateguard,
    edenfs,
    encoding,
    error,
    formatter,
    git,
    graphmod,
    identity,
    json,
    match as matchmod,
    mdiff,
    mergeutil,
    mutation,
    patch,
    pathutil,
    perftrace,
    progress,
    pycompat,
    registrar,
    revlog,
    scmutil,
    smartset,
    templatekw,
    templater,
    util,
    vfs as vfsmod,
)
from .i18n import _, _x
from .node import hex, nullid, nullrev, short
from .pycompat import ensureunicode, range


stringio = util.stringio

# templates of common command options


def _typedflags(flags):
    return flags


dryrunopts = [("n", "dry-run", None, _("do not perform actions, just print output"))]

walkopts = _typedflags(
    [
        (
            "I",
            "include",
            [],
            _("include files matching the given patterns"),
            _("PATTERN"),
        ),
        (
            "X",
            "exclude",
            [],
            _("exclude files matching the given patterns"),
            _("PATTERN"),
        ),
    ]
)

commitopts = [
    ("m", "message", "", _("use text as commit message"), _("TEXT")),
    ("l", "logfile", "", _("read commit message from file"), _("FILE")),
]

commitopts2 = [
    ("d", "date", "", _("record the specified date as commit date"), _("DATE")),
    ("u", "user", "", _("record the specified user as committer"), _("USER")),
]

# hidden for now
formatteropts = _typedflags(
    [("T", "template", "", _("display with template (EXPERIMENTAL)"), _("TEMPLATE"))]
)

templateopts = _typedflags(
    [
        (
            "",
            "style",
            "",
            _("display using template map file (DEPRECATED)"),
            _("STYLE"),
        ),
        ("T", "template", "", _("display with template"), _("TEMPLATE")),
    ]
)

logopts = (
    _typedflags(
        [
            ("p", "patch", None, _("show patch")),
            ("g", "git", None, _("use git extended diff format")),
            ("l", "limit", "", _("limit number of changes displayed"), _("NUM")),
            ("M", "no-merges", None, _("do not show merges")),
            ("", "stat", None, _("output diffstat-style summary of changes")),
            ("G", "graph", None, _("show the revision DAG")),
        ]
    )
    + templateopts
)

diffopts = [
    ("a", "text", None, _("treat all files as text")),
    ("g", "git", None, _("use git extended diff format")),
    ("", "binary", None, _("generate binary diffs in git mode (default)")),
    ("", "nodates", None, _("omit dates from diff headers")),
]

diffwsopts = _typedflags(
    [
        ("w", "ignore-all-space", None, _("ignore white space when comparing lines")),
        (
            "b",
            "ignore-space-change",
            None,
            _("ignore changes in the amount of white space"),
        ),
        (
            "B",
            "ignore-blank-lines",
            None,
            _("ignore changes whose lines are all blank"),
        ),
        ("Z", "ignore-space-at-eol", None, _("ignore changes in whitespace at EOL")),
    ]
)

diffopts2 = (
    _typedflags(
        [
            ("", "noprefix", None, _("omit a/ and b/ prefixes from filenames")),
            ("p", "show-function", None, _("show which function each change is in")),
            ("", "reverse", None, _("produce a diff that undoes the changes")),
        ]
    )
    + diffwsopts
    + _typedflags(
        [
            ("U", "unified", "", _("number of lines of context to show"), _("NUM")),
            ("", "stat", None, _("output diffstat-style summary of changes")),
            ("", "root", "", _("produce diffs relative to subdirectory"), _("DIR")),
            (
                "",
                "only-files-in-revs",
                None,
                _("only show changes for files modified in the requested revisions"),
            ),
        ]
    )
)

mergetoolopts = [("t", "tool", "", _("specify merge tool"))]

similarityopts = [
    (
        "s",
        "similarity",
        "",
        _("guess renamed files by similarity (0<=s<=100)"),
        _("SIMILARITY"),
    )
]

debugrevlogopts = [
    ("c", "changelog", False, _("open changelog")),
    ("m", "manifest", False, _("open manifest")),
    ("", "dir", "", _("open directory manifest")),
]

# special string such that everything below this line will be ingored in the
# editor text
_linebelow = (
    f"^{identity.tmplprefix()}: ------------------------ >8 ------------------------$"
)


def ishunk(x):
    hunkclasses = (crecordmod.uihunk, patch.recordhunk)
    return isinstance(x, hunkclasses)


def newandmodified(chunks, originalchunks):
    newlyaddedandmodifiedfiles = set()
    for chunk in chunks:
        if ishunk(chunk) and chunk.header.isnewfile() and chunk not in originalchunks:
            newlyaddedandmodifiedfiles.add(chunk.header.filename())
    return newlyaddedandmodifiedfiles


def extractcopies(chunks) -> "Dict[str, str]":
    result = {}
    for chunk in chunks:
        if ishunk(chunk):
            copyfrom = chunk.header.copyfrom()
            if copyfrom:
                copyto = chunk.header.filename()
                result[copyto] = copyfrom
    return result


def comparechunks(chunks, headers):
    """
    Determine whether the sets of chunks is the same as the original set of
    headers, after they had been filtered.

    Generate patches for both sets of data and then compare the patches.
    """

    originalpatch = stringio()
    for header in headers:
        header.write(originalpatch)
        for hunk in header.hunks:
            hunk.write(originalpatch)

    newpatch = stringio()
    for chunk in chunks:
        chunk.write(newpatch)

    return newpatch.getvalue() == originalpatch.getvalue()


def parsealiases(cmd):
    return cmd.lstrip("^").split("|")


def setupwrapcolorwrite(ui):
    # wrap ui.write so diff output can be labeled/colorized
    def wrapwritebytes(orig, *args, **kw):
        label = kw.pop(r"label", "")
        for chunk, l in patch.difflabel(lambda: args):
            orig(chunk, label=label + l)

    oldwrite = ui.writebytes

    def wrap(*args, **kwargs):
        return wrapwritebytes(oldwrite, *args, **kwargs)

    setattr(ui, "writebytes", wrap)
    return oldwrite


def filterchunks(ui, originalhunks, usecurses, testfile, operation=None):
    if usecurses:
        if testfile:
            recordfn = crecordmod.testdecorator(testfile, crecordmod.testchunkselector)
        else:
            recordfn = crecordmod.chunkselector

        return crecordmod.filterpatch(ui, originalhunks, recordfn, operation)

    else:
        return patch.filterpatch(ui, originalhunks, operation)


def recordfilter(ui, originalhunks, operation=None):
    """Prompts the user to filter the originalhunks and return a list of
    selected hunks.
    *operation* is used for to build ui messages to indicate the user what
    kind of filtering they are doing: reverting, committing, shelving, etc.
    (see patch.filterpatch).
    """
    usecurses = crecordmod.checkcurses(ui)
    testfile = ui.config("experimental", "crecordtest")
    oldwrite = setupwrapcolorwrite(ui)
    try:
        newchunks, newopts = filterchunks(
            ui, originalhunks, usecurses, testfile, operation
        )
    finally:
        ui.writebytes = oldwrite
    return newchunks, newopts


def dorecord(ui, repo, commitfunc, cmdsuggest, backupall, filterfn, *pats, **opts):
    from . import merge as mergemod

    if not ui.interactive():
        if cmdsuggest:
            msg = _("running non-interactively, use %s instead") % cmdsuggest
        else:
            msg = _("running non-interactively")
        raise error.Abort(msg)

    # make sure username is set before going interactive
    if not opts.get("user"):
        ui.username()  # raise exception, username not provided

    def recordfunc(ui, repo, message, match, opts):
        """This is generic record driver.

        Its job is to interactively filter local changes, and
        accordingly prepare working directory into a state in which the
        job can be delegated to a non-interactive commit command such as
        'commit' or 'qrefresh'.

        After the actual job is done by non-interactive command, the
        working directory is restored to its original state.

        In the end we'll record interesting changes, and everything else
        will be left in place, so the user can continue working.
        """

        checkunfinished(repo, commit=True)
        wctx = repo[None]
        merge = len(wctx.parents()) > 1
        if merge:
            raise error.Abort(
                _("cannot partially commit a merge " '(use "@prog@ commit" instead)')
            )

        def fail(f, msg):
            raise error.Abort("%s: %s" % (f, msg))

        force = opts.get("force")
        if not force:
            match.bad = fail

        status = repo.status(match=match)
        if not force:
            repo.checkcommitpatterns(wctx, match, status, fail)
        diffopts = patch.difffeatureopts(ui, opts=opts, whitespace=True)
        diffopts.nodates = True
        diffopts.git = True
        diffopts.showfunc = True
        originaldiff = patch.diff(
            repo, repo[repo.dirstate.p1()], repo[None], changes=status, opts=diffopts
        )
        originalchunks = patch.parsepatch(originaldiff)

        # 1. filter patch, since we are intending to apply subset of it
        try:
            chunks, newopts = filterfn(ui, originalchunks)
        except error.PatchError as err:
            raise error.Abort(_("error parsing patch: %s") % err)
        opts.update(newopts)

        # We need to keep a backup of files that have been newly added and
        # modified during the recording process because there is a previous
        # version without the edit in the workdir
        newlyaddedandmodifiedfiles = newandmodified(chunks, originalchunks)
        contenders = set()
        for h in chunks:
            try:
                contenders.update(set(h.files()))
            except AttributeError:
                pass
        changed = status.modified + status.added + status.removed
        newfiles = [f for f in changed if f in contenders]
        if not newfiles:
            ui.status(_("no changes to record\n"))
            return 0

        modified = set(status.modified)

        # 2. backup changed files, so we can restore them in the end

        if backupall:
            tobackup = changed
        else:
            tobackup = [
                f for f in newfiles if f in modified or f in newlyaddedandmodifiedfiles
            ]
        copied = extractcopies(chunks)
        tobackup += sorted(copied.keys())  # backup "copyto" - delete by step 3a
        tobackup += sorted(copied.values())  # backup "copyfrom" - rewrite by step 3a
        backups = {}
        if tobackup:
            backupdir = repo.localvfs.join("record-backups")
            try:
                os.mkdir(backupdir)
            except OSError as err:
                if err.errno != errno.EEXIST:
                    raise
        try:
            # backup continues
            for f in tobackup:
                if not repo.wvfs.exists(f):
                    continue
                fd, tmpname = tempfile.mkstemp(dir=backupdir)
                os.close(fd)
                ui.debug("backup %r as %r\n" % (f, tmpname))
                util.copyfile(repo.wjoin(f), tmpname, copystat=True)
                backups[f] = tmpname

            fp = stringio()
            for c in chunks:
                if c.filename() in backups:
                    c.write(fp)
            dopatch = fp.tell()
            fp.seek(0)

            # 2.5 optionally review / modify patch in text editor
            if opts.get("review", False):
                patchtext = (
                    crecordmod.diffhelptext
                    + crecordmod.patchhelptext
                    + pycompat.decodeutf8(fp.read())
                )
                reviewedpatch = ui.edit(
                    patchtext, "", action="diff", repopath=repo.path
                )
                fp.truncate(0)
                fp.write(pycompat.encodeutf8(reviewedpatch))
                fp.seek(0)

            [os.unlink(repo.wjoin(c)) for c in newlyaddedandmodifiedfiles]

            # 3a. Prepare "copyfrom" -> "copyto" files. Write "copyfrom"
            # and remove "copyto".
            # This is used by patch._applydiff. If _applydiff reads directly
            # from repo["."], not repo.wvfs, then this could be unnecessary.
            for copyto, copyfrom in copied.items():
                content = repo["."][copyfrom].data()
                repo.wvfs.write(copyfrom, content)
                repo.wvfs.tryunlink(copyto)

            # 3b. apply filtered patch to clean repo  (clean)
            if backups:
                # Equivalent to hg.revert
                m = scmutil.matchfiles(repo, backups.keys())
                mergemod.update(repo, repo.dirstate.p1(), False, True, matcher=m)

            # 3c. (apply)
            if dopatch:
                try:
                    ui.debug("applying patch\n")
                    patch.internalpatch(ui, repo, fp, 1, eolmode=None)
                except error.PatchError as err:
                    raise error.Abort(str(err))
            del fp

            # 4. We prepared working directory according to filtered
            #    patch. Now is the time to delegate the job to
            #    commit/qrefresh or the like!

            # Make all of the pathnames absolute.
            newfiles = [repo.wjoin(nf) for nf in newfiles]
            return commitfunc(ui, repo, *newfiles, **opts)
        finally:
            # 5. finally restore backed-up files
            try:
                dirstate = repo.dirstate
                for realname, tmpname in pycompat.iteritems(backups):
                    ui.debug("restoring %r to %r\n" % (tmpname, realname))

                    if dirstate[realname] == "n":
                        # without normallookup, restoring timestamp
                        # may cause partially committed files
                        # to be treated as unmodified
                        dirstate.normallookup(realname)

                    # copystat=True here and above are a hack to trick any
                    # editors that have f open that we haven't modified them.
                    #
                    # Also note that this racy as an editor could notice the
                    # file's mtime before we've finished writing it.
                    util.copyfile(tmpname, repo.wjoin(realname), copystat=True)
                    os.unlink(tmpname)
                if tobackup:
                    os.rmdir(backupdir)
            except OSError:
                pass

    def recordinwlock(ui, repo, message, match, opts):
        with repo.wlock():
            return recordfunc(ui, repo, message, match, opts)

    return commit(ui, repo, recordinwlock, pats, opts)


class dirnode(object):
    """
    Represent a directory in user working copy with information required for
    the purpose of tersing its status.

    path is the path to the directory

    statuses is a set of statuses of all files in this directory (this includes
    all the files in all the subdirectories too)

    files is a list of files which are direct child of this directory

    subdirs is a dictionary of sub-directory name as the key and it's own
    dirnode object as the value
    """

    def __init__(self, dirpath):
        self.path = dirpath
        self.statuses = set([])
        self.files = []
        self.subdirs = {}

    def _addfileindir(self, filename, status):
        """Add a file in this directory as a direct child."""
        self.files.append((filename, status))

    def addfile(self, filename, status):
        """
        Add a file to this directory or to its direct parent directory.

        If the file is not direct child of this directory, we traverse to the
        directory of which this file is a direct child of and add the file
        there.
        """

        # the filename contains a path separator, it means it's not the direct
        # child of this directory
        if "/" in filename:
            subdir, filep = filename.split("/", 1)

            # does the dirnode object for subdir exists
            if subdir not in self.subdirs:
                subdirpath = os.path.join(self.path, subdir)
                self.subdirs[subdir] = dirnode(subdirpath)

            # try adding the file in subdir
            self.subdirs[subdir].addfile(filep, status)

        else:
            self._addfileindir(filename, status)

        if status not in self.statuses:
            self.statuses.add(status)

    def iterfilepaths(self):
        """Yield (status, path) for files directly under this directory."""
        for f, st in self.files:
            yield st, os.path.join(self.path, f)

    def tersewalk(self, terseargs):
        """
        Yield (status, path) obtained by processing the status of this
        dirnode.

        terseargs is the string of arguments passed by the user with `--terse`
        flag.

        Following are the cases which can happen:

        1) All the files in the directory (including all the files in its
        subdirectories) share the same status and the user has asked us to terse
        that status. -> yield (status, dirpath)

        2) Otherwise, we do following:

                a) Yield (status, filepath)  for all the files which are in this
                    directory (only the ones in this directory, not the subdirs)

                b) Recurse the function on all the subdirectories of this
                   directory
        """

        if len(self.statuses) == 1:
            onlyst = self.statuses.pop()

            # Making sure we terse only when the status abbreviation is
            # passed as terse argument
            if onlyst in terseargs:
                yield onlyst, self.path + pycompat.ossep
                return

        # add the files to status list
        for st, fpath in self.iterfilepaths():
            yield st, fpath

        # recurse on the subdirs
        for dirobj in self.subdirs.values():
            for st, fpath in dirobj.tersewalk(terseargs):
                yield st, fpath


def tersedir(statuslist, terseargs):
    """
    Terse the status if all the files in a directory shares the same status.

    statuslist is scmutil.status() object which contains a list of files for
    each status.
    terseargs is string which is passed by the user as the argument to `--terse`
    flag.

    The function makes a tree of objects of dirnode class, and at each node it
    stores the information required to know whether we can terse a certain
    directory or not.
    """
    # the order matters here as that is used to produce final list
    allst = ("m", "a", "r", "d", "u", "i", "c")

    # checking the argument validity
    for s in pycompat.bytestr(terseargs):
        if s not in allst:
            raise error.Abort(_("'%s' not recognized") % s)

    # creating a dirnode object for the root of the repo
    rootobj = dirnode("")
    pstatus = ("modified", "added", "deleted", "clean", "unknown", "ignored", "removed")

    tersedict = {}
    for attrname in pstatus:
        statuschar = attrname[0:1]
        for f in getattr(statuslist, attrname):
            rootobj.addfile(f, statuschar)
        tersedict[statuschar] = []

    # we won't be tersing the root dir, so add files in it
    for st, fpath in rootobj.iterfilepaths():
        tersedict[st].append(fpath)

    # process each sub-directory and build tersedict
    for subdir in rootobj.subdirs.values():
        for st, f in subdir.tersewalk(terseargs):
            tersedict[st].append(f)

    tersedlist = []
    for st in allst:
        tersedict[st].sort()
        tersedlist.append(tersedict[st])

    return tersedlist


def _commentlines(raw):
    """Surround lineswith a comment char and a new line"""
    lines = raw.splitlines()
    commentedlines = ["# %s" % line for line in lines]
    return "\n".join(commentedlines) + "\n"


def _conflictsmsg(repo):
    # avoid merge cycle
    from . import merge as mergemod

    mergestate = mergemod.mergestate.read(repo)
    if not mergestate.active():
        return

    m = scmutil.match(repo[None])
    unresolvedlist = [f for f in mergestate.unresolved() if m(f)]
    if unresolvedlist:
        mergeliststr = "\n".join(
            [
                "    %s" % util.pathto(repo.root, pycompat.getcwd(), path)
                for path in unresolvedlist
            ]
        )
        msg = (
            _(
                """Unresolved merge conflicts:

%s

To mark files as resolved:  hg resolve --mark FILE"""
            )
            % mergeliststr
        )
    else:
        msg = _("No unresolved merge conflicts.")

    return _commentlines(msg)


def _helpmessage(continuecmd, abortcmd):
    msg = _("To continue:                %s\n" "To abort:                   %s") % (
        continuecmd,
        abortcmd,
    )
    return _commentlines(msg)


def _rebasemsg():
    return _helpmessage(_("@prog@ rebase --continue"), _("@prog@ rebase --abort"))


def _histeditmsg():
    return _helpmessage(_("@prog@ histedit --continue"), _("@prog@ histedit --abort"))


def _unshelvemsg():
    return _helpmessage(_("@prog@ unshelve --continue"), _("@prog@ unshelve --abort"))


def _updatecleanmsg(dest=None):
    warning = _("warning: this will discard uncommitted changes")
    return _("@prog@ goto --clean %s    (%s)") % (dest or ".", warning)


def _graftmsg():
    # tweakdefaults requires `update` to have a rev hence the `.`
    return _helpmessage(_("@prog@ graft --continue"), _updatecleanmsg())


def _mergemsg():
    # tweakdefaults requires `update` to have a rev hence the `.`
    return _helpmessage(_("@prog@ commit"), _updatecleanmsg())


def _bisectmsg():
    msg = _(
        "To mark the changeset good:    @prog@ bisect --good\n"
        "To mark the changeset bad:     @prog@ bisect --bad\n"
        "To abort:                      @prog@ bisect --reset\n"
    )
    return _commentlines(msg)


def fileexistspredicate(filename):
    return lambda repo: repo.localvfs.exists(filename)


def _mergepredicate(repo):
    return len(repo[None].parents()) > 1


STATES = (
    # (state, predicate to detect states, helpful message function)
    ("histedit", fileexistspredicate("histedit-state"), _histeditmsg),
    ("bisect", fileexistspredicate("bisect.state"), _bisectmsg),
    ("graft", fileexistspredicate("graftstate"), _graftmsg),
    ("unshelve", fileexistspredicate("unshelverebasestate"), _unshelvemsg),
    ("rebase", fileexistspredicate("rebasestate"), _rebasemsg),
    # The merge state is part of a list that will be iterated over.
    # They need to be last because some of the other unfinished states may also
    # be in a merge or update state (eg. rebase, histedit, graft, etc).
    # We want those to have priority.
    ("merge", _mergepredicate, _mergemsg),
)


def _getrepostate(repo):
    # experimental config: commands.status.skipstates
    skip = set(repo.ui.configlist("commands", "status.skipstates"))
    for state, statedetectionpredicate, msgfn in STATES:
        if state in skip:
            continue
        if statedetectionpredicate(repo):
            return (state, statedetectionpredicate, msgfn)


def morestatus(repo, fm):
    statetuple = _getrepostate(repo)
    label = "status.morestatus"
    if statetuple:
        fm.startitem()
        state, statedetectionpredicate, helpfulmsg = statetuple
        statemsg = _("The repository is in an unfinished *%s* state.") % state
        fm.write("statemsg", "%s\n", _commentlines(statemsg), label=label)
        conmsg = _conflictsmsg(repo)
        if conmsg:
            fm.write("conflictsmsg", "%s\n", conmsg, label=label)
        if helpfulmsg:
            helpmsg = helpfulmsg()
            fm.write("helpmsg", "%s\n", helpmsg, label=label)


def findpossible(cmd, table):
    """
    Return cmd -> (aliases, command table entry)
    for each matching command.
    Return debug commands (or their aliases) only if no normal command matches.
    """
    choice = {}
    debugchoice = {}

    if cmd in table:
        # short-circuit exact matches, "log" alias beats "^log|history"
        keys = [cmd]
    else:
        keys = table.keys()

    allcmds = []
    for e in keys:
        aliases = parsealiases(e)
        allcmds.extend(aliases)
        found = None
        if cmd in aliases:
            found = cmd
        if found is not None:
            if aliases[0].startswith("debug") or found.startswith("debug"):
                debugchoice[found] = (aliases, table[e])
            else:
                choice[found] = (aliases, table[e])

    if not choice and debugchoice:
        choice = debugchoice

    return choice, allcmds


def getcmdanddefaultopts(cmdname, table):
    """Returns (command, defaultopts) for cmd string
    This function returns command and all the default options already
    initialized. It should be used by commands that call other commands. For
    example, calling pull inside of update, calling log inside of show etc.
    getcmdanddefaultopts has important benefits:
    1) It returns "wrapped" command i.e. command with all the overrides applied.
    This is better than calling commands.pull() directly.
    2) getcmdanddefaultopts correctly initializes options to their default value
    and correctly changes their name - replace '-'  with '_'.
    """

    cmdname = cmdname.split()
    for subcmd in cmdname:
        _aliases, cmdwithopts = findcmd(subcmd, table)
        table = cmdwithopts[0].subcommands

    cmd, optsdescription = cmdwithopts[:2]

    opts = {}
    for opt in optsdescription:
        name = opt[1].replace("-", "_")
        value = opt[2]
        opts[name] = value

    return (cmd, opts)


def findcmd(cmd, table):
    """Return (aliases, command table entry) for command string."""
    choice, allcmds = findpossible(cmd, table)

    if cmd in choice:
        return choice[cmd]

    if len(choice) > 1:
        clist = sorted(choice)
        raise error.AmbiguousCommand(cmd, clist)

    if choice:
        return list(choice.values())[0]

    raise error.UnknownCommand(cmd)


def findsubcmd(args, table, partial=False):
    cmd, args, level = args[0], args[1:], 1
    aliases, entry = findcmd(cmd, table)
    cmd = aliases[0]
    while args and entry[0] and util.safehasattr(entry[0], "subcommands"):
        try:
            subaliases, subentry = findcmd(args[0], entry[0].subcommands)
        except error.UnknownCommand as e:
            if entry[0].subonly:
                raise error.UnknownSubcommand(cmd, *e.args)
            else:
                break
        else:
            aliases, entry = subaliases, subentry
            cmd, args, level = "%s %s" % (cmd, aliases[0]), args[1:], level + 1
    if not partial and util.safehasattr(entry[0], "subonly") and entry[0].subonly:
        raise error.UnknownSubcommand(cmd, None, None)
    return cmd, args, aliases, entry, level


def findrepo(p):
    root = identity.sniffroot(p)
    if root:
        return root[0]

    return None


def uncommittedchanges(repo):
    """Returns if there are uncommitted changes"""
    modified, added, removed, deleted = repo.status()[:4]
    return modified or added or removed or deleted


def bailifchanged(repo, merge=True, hint=None):
    """enforce the precondition that working directory must be clean.

    'merge' can be set to false if a pending uncommitted merge should be
    ignored (such as when 'update --check' runs).

    'hint' is the usual hint given to Abort exception.
    """
    if merge and repo.dirstate.p2() != nullid:
        raise error.Abort(_("outstanding uncommitted merge"), hint=hint)

    if uncommittedchanges(repo):
        raise error.UncommitedChangesAbort(_("uncommitted changes"), hint=hint)


def logmessage(repo, opts):
    """get the log message according to -m and -l option"""
    ui = repo.ui

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
        opts["reuse_message"] = False

    message = opts.get("message")
    logfile = opts.get("logfile")

    if message and logfile:
        raise error.Abort(
            _("options --message and --logfile are mutually " "exclusive")
        )
    if not message and logfile:
        try:
            if isstdiofilename(logfile):
                message = pycompat.decodeutf8(ui.fin.read())
            else:
                message = pycompat.decodeutf8(
                    b"\n".join(util.readfile(logfile).splitlines())
                )
        except IOError as inst:
            raise error.Abort(
                _("can't read commit message '%s': %s")
                % (logfile, encoding.strtolocal(inst.strerror))
            )
    return message


def mergeeditform(ctxorbool, baseformname):
    """return appropriate editform name (referencing a committemplate)

    'ctxorbool' is either a ctx to be committed, or a bool indicating whether
    merging is committed.

    This returns baseformname with '.merge' appended if it is a merge,
    otherwise '.normal' is appended.
    """
    if isinstance(ctxorbool, bool):
        if ctxorbool:
            return baseformname + ".merge"
    elif 1 < len(ctxorbool.parents()):
        return baseformname + ".merge"

    return baseformname + ".normal"


def getcommiteditor(edit=False, finishdesc=None, extramsg=None, editform="", **opts):
    """get appropriate commit message editor according to '--edit' option

    'finishdesc' is a function to be called with edited commit message
    (= 'description' of the new changeset) just after editing, but
    before checking empty-ness. It should return actual text to be
    stored into history. This allows to change description before
    storing.

    'extramsg' is a extra message to be shown in the editor instead of
    'Leave message empty to abort commit' line. 'HG: ' prefix and EOL
    is automatically added.

    'editform' is a dot-separated list of names, to distinguish
    the purpose of commit text editing.

    'getcommiteditor' returns 'commitforceeditor' regardless of
    'edit', if one of 'finishdesc' or 'extramsg' is specified, because
    they are specific for usage in MQ.
    """
    if edit or finishdesc or extramsg:
        return lambda r, c: commitforceeditor(
            r, c, finishdesc=finishdesc, extramsg=extramsg, editform=editform
        )
    elif editform:
        return lambda r, c: commiteditor(r, c, editform=editform)
    else:
        return commiteditor


def loglimit(opts):
    """get the log limit according to option -l/--limit"""
    limit = opts.get("limit")
    if limit:
        try:
            limit = int(limit)
        except ValueError:
            raise error.Abort(_("limit must be a positive integer"))
        if limit <= 0:
            raise error.Abort(_("limit must be positive"))
    else:
        limit = None
    return limit


def makefilename(
    repo, pat, node, desc=None, total=None, seqno=None, revwidth=None, pathname=None
):
    node_expander = {
        "H": lambda: hex(node),
        "R": lambda: "%d" % repo.changelog.rev(node),
        "h": lambda: short(node),
        "m": lambda: re.sub(r"[^\w]", "_", desc or ""),
    }
    expander = {"%": lambda: "%", "b": lambda: os.path.basename(repo.root)}

    try:
        if node:
            expander.update(node_expander)
        if node:
            expander["r"] = lambda: ("%d" % repo.changelog.rev(node)).zfill(
                revwidth or 0
            )
        if total is not None:
            expander["N"] = lambda: "%d" % total
        if seqno is not None:
            expander["n"] = lambda: "%d" % seqno
        if total is not None and seqno is not None:
            expander["n"] = lambda: ("%d" % seqno).zfill(len("%d" % total))
        if pathname is not None:
            expander["s"] = lambda: os.path.basename(pathname)
            expander["d"] = lambda: os.path.dirname(pathname) or "."
            expander["p"] = lambda: pathname

        newname = []
        patlen = len(pat)
        i = 0
        while i < patlen:
            c = pat[i : i + 1]
            if c == "%":
                i += 1
                c = pat[i : i + 1]
                c = expander[c]()
            newname.append(c)
            i += 1
        return "".join(newname)
    except KeyError as inst:
        raise error.Abort(
            _("invalid format spec '%%%s' in output filename") % inst.args[0]
        )


def isstdiofilename(pat):
    """True if the given pat looks like a filename denoting stdin/stdout"""
    return not pat or pat == "-"


def rendertemplate(ui, tmpl, props=None):
    """Render tmpl written in the template language. props provides the
    "environment" which the template program runs in.

    Return the rendered string.

    >>> import edenscm.ui as uimod
    >>> rendertemplate(uimod.ui(), '{a} {b|json}', {'a': 'x', 'b': [3, None]})
    'x [3, null]'
    """
    t = formatter.maketemplater(ui, tmpl, cache=templatekw.defaulttempl)
    mapping = {"ui": ui, "templ": t}
    if props:
        if "ctx" in props:
            mapping["revcache"] = {}
        mapping.update(props)
    mapping.update(templatekw.keywords)
    return t.render(mapping)


class _unclosablefile(object):
    def __init__(self, fp):
        self._fp = fp

    def close(self):
        pass

    def __iter__(self):
        return iter(self._fp)

    def __getattr__(self, attr):
        return getattr(self._fp, attr)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        pass


def makefileobj(
    repo,
    pat,
    node=None,
    desc=None,
    total=None,
    seqno=None,
    revwidth=None,
    mode="wb",
    modemap=None,
    pathname=None,
):

    writable = mode not in ("r", "rb")

    if isstdiofilename(pat):
        if writable:
            fp = repo.ui.fout
        else:
            fp = repo.ui.fin
        return _unclosablefile(fp)
    fn = makefilename(repo, pat, node, desc, total, seqno, revwidth, pathname)
    if modemap is not None:
        mode = modemap.get(fn, mode)
        if mode == "wb":
            modemap[fn] = "ab"
    return open(fn, mode)


def openrevlog(repo, cmd, file_, opts):
    """opens the changelog, manifest, a filelog or a given revlog"""
    cl = opts["changelog"]
    mf = opts["manifest"]
    dir = opts["dir"]
    msg = None
    if cl and mf:
        msg = _("cannot specify --changelog and --manifest at the same time")
    elif cl and dir:
        msg = _("cannot specify --changelog and --dir at the same time")
    elif cl or mf or dir:
        if file_:
            msg = _("cannot specify filename with --changelog or --manifest")
        elif not repo:
            msg = _(
                "cannot specify --changelog or --manifest or --dir "
                "without a repository"
            )
    if msg:
        raise error.Abort(msg)

    r = None
    if repo:
        if cl:
            cl = repo.changelog
            r = revlog.revlog(cl.opener, cl.indexfile)
        elif dir:
            if "treemanifest" not in repo.requirements:
                raise error.Abort(
                    _("--dir can only be used on repos with " "treemanifest enabled")
                )
            dirlog = repo.manifestlog._revlog.dirlog(dir)
            if len(dirlog):
                r = dirlog
        elif mf:
            r = repo.manifestlog._revlog
        elif file_:
            filelog = repo.file(file_)
            if len(filelog):
                r = filelog
    if not r:
        if not file_:
            raise error.CommandError(cmd, _("invalid arguments"))
        if not os.path.isfile(file_):
            raise error.Abort(_("revlog '%s' not found") % file_)
        r = revlog.revlog(vfsmod.vfs(pycompat.getcwd(), audit=False), file_[:-2] + ".i")
    return r


def copy(ui, repo, pats, opts, rename=False):
    # called with the repo lock held
    #
    # hgsep => pathname that uses "/" to separate directories
    # ossep => pathname that uses os.sep to separate directories
    cwd = repo.getcwd()
    targets = {}
    after = opts.get("after")
    dryrun = opts.get("dry_run")
    wctx = repo[None]

    def walkpat(pat):
        srcs = []
        if after:
            badstates = "?"
        else:
            badstates = "?r"
        m = scmutil.match(wctx, [pat], opts, globbed=True)
        for abs in wctx.walk(m):
            state = repo.dirstate[abs]
            rel = m.rel(abs)
            exact = m.exact(abs)
            if state in badstates:
                if exact and state == "?":
                    ui.warn(_("%s: not copying - file is not managed\n") % rel)
                if exact and state == "r":
                    ui.warn(
                        _("%s: not copying - file has been marked for" " remove\n")
                        % rel
                    )
                continue
            # abs: hgsep
            # rel: ossep
            srcs.append((abs, rel, exact))
        return srcs

    # abssrc: hgsep
    # relsrc: ossep
    # otarget: ossep
    def copyfile(abssrc, relsrc, otarget, exact):
        abstarget = pathutil.canonpath(repo.root, cwd, otarget)
        if "/" in abstarget:
            # We cannot normalize abstarget itself, this would prevent
            # case only renames, like a => A.
            abspath, absname = abstarget.rsplit("/", 1)
            abstarget = repo.dirstate.normalize(abspath) + "/" + absname
        reltarget = repo.pathto(abstarget, cwd)
        target = repo.wjoin(abstarget)
        src = repo.wjoin(abssrc)
        state = repo.dirstate[abstarget]

        scmutil.checkportable(ui, abstarget)

        # check for collisions
        prevsrc = targets.get(abstarget)
        if prevsrc is not None:
            ui.warn(
                _("%s: not overwriting - %s collides with %s\n")
                % (reltarget, repo.pathto(abssrc, cwd), repo.pathto(prevsrc, cwd))
            )
            return

        # check for overwrites
        exists = os.path.lexists(target)
        samefile = False
        if exists and abssrc != abstarget:
            if repo.dirstate.normalize(abssrc) == repo.dirstate.normalize(abstarget):
                if not rename:
                    ui.warn(_("%s: can't copy - same file\n") % reltarget)
                    return
                exists = False
                samefile = True

        if not after and exists or after and state in "mn":
            if not opts["force"]:
                if state in "mn":
                    msg = _("%s: not overwriting - file already committed\n")
                    if after:
                        flags = "--after --force"
                    else:
                        flags = "--force"
                    if rename:
                        hint = (
                            _(
                                "(@prog@ rename %s to replace the file by "
                                "recording a rename)\n"
                            )
                            % flags
                        )
                    else:
                        hint = (
                            _(
                                "(@prog@ copy %s to replace the file by "
                                "recording a copy)\n"
                            )
                            % flags
                        )
                else:
                    msg = _("%s: not overwriting - file exists\n")
                    if rename:
                        hint = _("(@prog@ rename --after to record the rename)\n")
                    else:
                        hint = _("(@prog@ copy --after to record the copy)\n")
                ui.warn(msg % reltarget)
                ui.warn(hint)
                return

        if after:
            if not exists:
                if rename:
                    ui.warn(
                        _("%s: not recording move - %s does not exist\n")
                        % (relsrc, reltarget)
                    )
                else:
                    ui.warn(
                        _("%s: not recording copy - %s does not exist\n")
                        % (relsrc, reltarget)
                    )
                return
        elif not dryrun:
            try:
                if exists:
                    os.unlink(target)
                targetdir = os.path.dirname(target) or "."
                if not os.path.isdir(targetdir):
                    os.makedirs(targetdir)
                if samefile:
                    tmp = target + "~hgrename"
                    os.rename(src, tmp)
                    os.rename(tmp, target)
                else:
                    util.copyfile(src, target)
                srcexists = True
            except IOError as inst:
                if inst.errno == errno.ENOENT:
                    ui.warn(_("%s: deleted in working directory\n") % relsrc)
                    srcexists = False
                else:
                    ui.warn(
                        _("%s: cannot copy - %s\n")
                        % (relsrc, encoding.strtolocal(inst.strerror))
                    )
                    return True  # report a failure

        if ui.verbose or not exact:
            if rename:
                ui.status(_("moving %s to %s\n") % (relsrc, reltarget))
            else:
                ui.status(_("copying %s to %s\n") % (relsrc, reltarget))

        targets[abstarget] = abssrc

        # fix up dirstate
        scmutil.dirstatecopy(ui, repo, wctx, abssrc, abstarget, dryrun=dryrun, cwd=cwd)
        if rename and not dryrun:
            if not after and srcexists and not samefile:
                repo.wvfs.unlinkpath(abssrc)
            wctx.forget([abssrc])

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathfn(pat, dest, srcs):
        if os.path.isdir(pat):
            abspfx = pathutil.canonpath(repo.root, cwd, pat)
            abspfx = util.localpath(abspfx)
            if destdirexists:
                striplen = len(os.path.split(abspfx)[0])
            else:
                striplen = len(abspfx)
            if striplen:
                striplen += len(pycompat.ossep)
            res = lambda p: os.path.join(dest, util.localpath(p)[striplen:])
        elif destdirexists:
            res = lambda p: os.path.join(dest, os.path.basename(util.localpath(p)))
        else:
            res = lambda p: dest
        return res

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathafterfn(pat, dest, srcs):
        if matchmod.patkind(pat):
            # a mercurial pattern
            res = lambda p: os.path.join(dest, os.path.basename(util.localpath(p)))
        else:
            abspfx = pathutil.canonpath(repo.root, cwd, pat)
            if len(abspfx) < len(srcs[0][0]):
                # A directory. Either the target path contains the last
                # component of the source path or it does not.
                def evalpath(striplen):
                    score = 0
                    for s in srcs:
                        t = os.path.join(dest, util.localpath(s[0])[striplen:])
                        if os.path.lexists(t):
                            score += 1
                    return score

                abspfx = util.localpath(abspfx)
                striplen = len(abspfx)
                if striplen:
                    striplen += len(pycompat.ossep)
                if os.path.isdir(os.path.join(dest, os.path.split(abspfx)[1])):
                    score = evalpath(striplen)
                    striplen1 = len(os.path.split(abspfx)[0])
                    if striplen1:
                        striplen1 += len(pycompat.ossep)
                    if evalpath(striplen1) > score:
                        striplen = striplen1
                res = lambda p: os.path.join(dest, util.localpath(p)[striplen:])
            else:
                # a file
                if destdirexists:
                    res = lambda p: os.path.join(
                        dest, os.path.basename(util.localpath(p))
                    )
                else:
                    res = lambda p: dest
        return res

    pats = scmutil.expandpats(pats)
    if not pats:
        raise error.Abort(_("no source or destination specified"))
    if len(pats) == 1:
        raise error.Abort(_("no destination specified"))
    dest = pats.pop()
    destdirexists = os.path.isdir(dest) and not os.path.islink(dest)
    if not destdirexists:
        if len(pats) > 1 or matchmod.patkind(pats[0]):
            raise error.Abort(
                _("with multiple sources, destination must be an " "existing directory")
            )
        if util.endswithsep(dest):
            raise error.Abort(_("destination %s is not a directory") % dest)

    tfn = targetpathfn
    if after:
        tfn = targetpathafterfn
    copylist = []
    for pat in pats:
        srcs = walkpat(pat)
        if not srcs:
            continue
        copylist.append((tfn(pat, dest, srcs), srcs))
    if not copylist:
        raise error.Abort(_("no files to copy"))

    errors = 0
    for targetpath, srcs in copylist:
        for abssrc, relsrc, exact in srcs:
            if copyfile(abssrc, relsrc, targetpath(abssrc), exact):
                errors += 1

    if errors:
        ui.warn(_("(consider using --after)\n"))

    return errors != 0


def uncopy(ui, repo, matcher, opts):
    # called with the repo lock held
    ret = 1  # return 1 if nothing changed
    dryrun = opts.get("dry_run")
    status = repo.status(match=matcher)
    matches = sorted(status.modified + status.added)
    for fname in matches:
        if ui.verbose:
            ui.status(_("uncopying %s") % (fname,))
        if not dryrun:
            ret = 0
            repo.dirstate.copy(None, fname)
    return ret


## facility to let extension process additional data into an import patch
# list of identifier to be executed in order
extrapreimport = []  # run before commit
extrapostimport = []  # run after commit
# mapping from identifier to actual import function
#
# 'preimport' are run before the commit is made and are provided the following
# arguments:
# - repo: the localrepository instance,
# - patchdata: data extracted from patch header (cf m.patch.patchheadermap),
# - extra: the future extra dictionary of the changeset, please mutate it,
# - opts: the import options.
# XXX ideally, we would just pass an ctx ready to be computed, that would allow
# mutation of in memory commit and more. Feel free to rework the code to get
# there.
extrapreimportmap = {}
# 'postimport' are run after the commit is made and are provided the following
# argument:
# - ctx: the changectx created by import.
extrapostimportmap = {}


def tryimportone(ui, repo, hunk, parents, opts, msgs, updatefunc):
    """Utility function used by commands.import to import a single patch

    This function is explicitly defined here to help the evolve extension to
    wrap this part of the import logic.

    The API is currently a bit ugly because it a simple code translation from
    the import command. Feel free to make it better.

    :hunk: a patch (as a binary string)
    :parents: nodes that will be parent of the created commit
    :opts: the full dict of option passed to the import command
    :msgs: list to save commit message to.
           (used in case we need to save it when failing)
    :updatefunc: a function that update a repo to a given node
                 updatefunc(<repo>, <node>)
    """
    # avoid cycle context -> cmdutil
    from . import context

    extractdata = patch.extract(ui, hunk)
    tmpname = extractdata.get("filename")
    message = extractdata.get("message")
    user = opts.get("user") or extractdata.get("user")
    date = opts.get("date") or extractdata.get("date")
    branch = extractdata.get("branch")
    nodeid = extractdata.get("nodeid")
    p1 = extractdata.get("p1")
    p2 = extractdata.get("p2")

    nocommit = opts.get("no_commit")
    update = not opts.get("bypass")
    strip = opts["strip"]
    prefix = opts["prefix"]
    sim = float(opts.get("similarity") or 0)
    if not tmpname:
        return (None, None, False)

    rejects = False

    try:
        cmdline_message = logmessage(repo, opts)
        if cmdline_message:
            # pickup the cmdline msg
            message = cmdline_message
        elif message:
            # pickup the patch msg
            message = message.strip()
        else:
            # launch the editor
            message = None
        ui.debug("message:\n%s\n" % message)

        if len(parents) == 1:
            parents.append(repo[nullid])
        if opts.get("exact"):
            if not nodeid or not p1:
                raise error.Abort(_("not a @Product@ patch"))
            p1 = repo[p1]
            p2 = repo[p2 or nullid]
        elif p2:
            try:
                p1 = repo[p1]
                p2 = repo[p2]
                # Without any options, consider p2 only if the
                # patch is being applied on top of the recorded
                # first parent.
                if p1 != parents[0]:
                    p1 = parents[0]
                    p2 = repo[nullid]
            except error.RepoError:
                p1, p2 = parents
            if p2.node() == nullid:
                ui.warn(
                    _(
                        "warning: import the patch as a normal revision\n"
                        "(use --exact to import the patch as a merge)\n"
                    )
                )
        else:
            p1, p2 = parents

        n = None
        if update:
            if p1 != parents[0]:
                updatefunc(repo, p1.node())
            if p2 != parents[1]:
                repo.setparents(p1.node(), p2.node())

            if opts.get("exact"):
                repo.dirstate.setbranch(branch or "default")

            partial = opts.get("partial", False)
            files = set()
            try:
                patch.patch(
                    ui,
                    repo,
                    tmpname,
                    strip=strip,
                    prefix=prefix,
                    files=files,
                    eolmode=None,
                    similarity=sim / 100.0,
                )
            except error.PatchError as e:
                if not partial:
                    raise error.Abort(str(e))
                if partial:
                    rejects = True

            files = list(files)
            if nocommit:
                if message:
                    msgs.append(message)
            else:
                if opts.get("exact") or p2:
                    # If you got here, you either use --force and know what
                    # you are doing or used --exact or a merge patch while
                    # being updated to its first parent.
                    m = None
                else:
                    m = scmutil.matchfiles(repo, files or [])
                editform = mergeeditform(repo[None], "import.normal")
                if opts.get("exact"):
                    editor = None
                else:
                    editor = getcommiteditor(editform=editform, **opts)
                extra = {}
                for idfunc in extrapreimport:
                    extrapreimportmap[idfunc](repo, extractdata, extra, opts)
                overrides = {}
                if partial:
                    overrides[("ui", "allowemptycommit")] = True
                with repo.ui.configoverride(overrides, "import"):
                    n = repo.commit(
                        message, user, date, match=m, editor=editor, extra=extra
                    )
                    for idfunc in extrapostimport:
                        extrapostimportmap[idfunc](repo[n])
        else:
            if opts.get("exact"):
                branch = branch or "default"
            else:
                branch = p1.branch()
            store = patch.filestore()
            try:
                files = set()
                try:
                    patch.patchrepo(
                        ui, repo, p1, store, tmpname, strip, prefix, files, eolmode=None
                    )
                except error.PatchError as e:
                    raise error.Abort(str(e))
                if opts.get("exact"):
                    editor = None
                else:
                    editor = getcommiteditor(editform="import.bypass")
                memctx = context.memctx(
                    repo,
                    (p1, p2),
                    message,
                    files=files,
                    filectxfn=store,
                    user=user,
                    date=date,
                    branch=branch,
                    editor=editor,
                )
                n = memctx.commit()
            finally:
                store.close()
        if opts.get("exact") and nocommit:
            # --exact with --no-commit is still useful in that it does merge
            # and branch bits
            ui.warn(_("warning: can't check exact import with --no-commit\n"))
        elif opts.get("exact") and hex(n) != nodeid:
            # Write the commit out. The "Abort" should not cancel the transaction.
            # This is the behavior tested by test-import-merge.t (issue3616).
            # It's a questionable behavior, though.
            tr = repo.currenttransaction()
            tr.close()
            raise error.Abort(_("patch is damaged or loses information"))
        msg = _("applied to working directory")
        if n:
            # i18n: refers to a short changeset id
            msg = _("created %s") % short(n)
        return (msg, n, rejects)
    finally:
        os.unlink(tmpname)


# facility to let extensions include additional data in an exported patch
# list of identifiers to be executed in order
extraexport = []
# mapping from identifier to actual export function
# function as to return a string to be added to the header or None
# it is given two arguments (sequencenumber, changectx)
extraexportmap = {}


def _exportsingle(
    repo, ctx, match, switch_parent, rev, seqno, write, diffopts, writestr=None
):
    if writestr is None:

        def writestr(s):
            write(pycompat.encodeutf8(s))

    node = scmutil.binnode(ctx)
    parents = [p.node() for p in ctx.parents() if p]
    branch = ctx.branch()
    if switch_parent:
        parents.reverse()

    if parents:
        prev = parents[0]
    else:
        prev = nullid

    writestr(f"# {identity.tmplprefix()} changeset patch\n")
    writestr("# User %s\n" % ctx.user())
    writestr("# Date %d %d\n" % ctx.date())
    writestr("#      %s\n" % util.datestr(ctx.date()))
    if branch and branch != "default":
        writestr("# Branch %s\n" % branch)
    writestr("# Node ID %s\n" % hex(node))
    writestr("# Parent  %s\n" % hex(prev))
    if len(parents) > 1:
        writestr("# Parent  %s\n" % hex(parents[1]))

    for headerid in extraexport:
        header = extraexportmap[headerid](seqno, ctx)
        if header is not None:
            writestr("# %s\n" % header)
    writestr(ctx.description().rstrip())
    writestr("\n\n")

    for chunk, label in patch.diffui(
        repo, repo[prev], repo[node], match, opts=diffopts
    ):
        write(chunk, label=label)


def export(
    repo,
    revs,
    fntemplate="hg-%h.patch",
    fp=None,
    switch_parent=False,
    opts=None,
    match=None,
):
    """export changesets as hg patches

    Args:
      repo: The repository from which we're exporting revisions.
      revs: A list of revisions to export as revision numbers.
      fntemplate: An optional string to use for generating patch file names.
      fp: An optional file-like object to which patches should be written.
      switch_parent: If True, show diffs against second parent when not nullid.
                     Default is false, which always shows diff against p1.
      opts: diff options to use for generating the patch.
      match: If specified, only export changes to files matching this matcher.

    Returns:
      Nothing.

    Side Effect:
      "HG Changeset Patch" data is emitted to one of the following
      destinations:
        fp is specified: All revs are written to the specified
                         file-like object.
        fntemplate specified: Each rev is written to a unique file named using
                            the given template.
        Neither fp nor template specified: All revs written to repo.ui.write()
    """

    total = len(revs)
    revwidth = max(len(str(rev)) for rev in revs)
    filemode = {}

    write = None
    writestr = None
    dest = "<unnamed>"
    if fp:
        dest = getattr(fp, "name", dest)

        def write(s, **kw):
            fp.write(s)

    elif not fntemplate:
        write = repo.ui.writebytes
        writestr = repo.ui.write

    for seqno, rev in enumerate(revs, 1):
        ctx = repo[rev]
        fo = None
        if not fp and fntemplate:
            desc_lines = ctx.description().rstrip().split("\n")
            desc = desc_lines[0]  # Commit always has a first line.
            fo = makefileobj(
                repo,
                fntemplate,
                ctx.node(),
                desc=desc,
                total=total,
                seqno=seqno,
                revwidth=revwidth,
                mode="wb",
                modemap=filemode,
            )
            dest = getattr(fo, "name", "<unnamed>")

            def write(s, **kw):
                fo.write(s)

        if not dest.startswith("<"):
            repo.ui.note("%s\n" % dest)
        _exportsingle(
            repo, ctx, match, switch_parent, rev, seqno, write, opts, writestr
        )
        if fo is not None:
            fo.close()


def diffordiffstat(
    ui,
    repo,
    diffopts,
    ctx1,
    ctx2,
    match,
    changes=None,
    stat=False,
    fp=None,
    prefix="",
    root="",
    hunksfilterfn=None,
):
    """show diff or diffstat."""
    if fp is None:
        if stat:
            write = ui.write
        else:
            write = ui.writebytes
    else:

        def write(s, **kw):
            fp.write(s)

    if root:
        relroot = pathutil.canonpath(repo.root, repo.getcwd(), root)
    else:
        relroot = ""
    if relroot != "":
        # XXX relative roots currently don't work if the root is within a
        # subrepo
        uirelroot = match.uipath(relroot)
        relroot += "/"
        for matchroot in match.files():
            if not matchroot.startswith(relroot):
                ui.warn(
                    _("warning: %s not inside relative root %s\n")
                    % (match.uipath(matchroot), uirelroot)
                )

    if stat:
        diffopts = diffopts.copy(context=0, noprefix=False)
        width = 80
        if not ui.plain():
            width = ui.termwidth()
        chunks = patch.diff(
            repo,
            ctx1,
            ctx2,
            match,
            changes,
            opts=diffopts,
            prefix=prefix,
            relroot=relroot,
            hunksfilterfn=hunksfilterfn,
        )
        for chunk, label in patch.diffstatui(util.iterlines(chunks), width=width):
            write(chunk, label=label)
    else:
        for chunk, label in patch.diffui(
            repo,
            ctx1,
            ctx2,
            match,
            changes,
            opts=diffopts,
            prefix=prefix,
            relroot=relroot,
            hunksfilterfn=hunksfilterfn,
        ):
            write(chunk, label=label)


def _changesetlabels(ctx):
    labels = ["log.changeset", "changeset.%s" % ctx.phasestr()]
    if ctx.obsolete():
        labels.append("changeset.obsolete")
    return " ".join(labels)


class changeset_printer(object):
    """show changeset information when templating not requested."""

    def __init__(self, ui, repo, matchfn, diffopts, buffered):
        self.ui = ui
        self.repo = repo
        self.buffered = buffered
        self.matchfn = matchfn
        self.diffopts = diffopts
        self.header = {}
        self.hunk = {}
        self.lastheader = None
        self.footer = None
        self._columns = templatekw.getlogcolumns()

    def flush(self, ctx):
        rev = ctx.rev()
        if rev in self.header:
            h = self.header[rev]
            if h != self.lastheader:
                self.lastheader = h
                self.ui.write(h)
            del self.header[rev]
        if rev in self.hunk:
            for elem in self.hunk[rev]:
                if isinstance(elem, str):
                    self.ui.write(elem)
                else:
                    self.ui.writebytes(elem)
            del self.hunk[rev]
            return 1
        return 0

    def close(self):
        if self.footer:
            self.ui.write(self.footer)

    def show(self, ctx, copies=None, matchfn=None, hunksfilterfn=None, **props):
        props = props
        if self.buffered:
            self.ui.pushbuffer(labeled=True)
            self._show(ctx, copies, matchfn, hunksfilterfn, props)
            self.hunk[ctx.rev()] = self.ui.popbufferlist()
        else:
            self._show(ctx, copies, matchfn, hunksfilterfn, props)

    def _show(self, ctx, copies, matchfn, hunksfilterfn, props):
        """show a single changeset or file revision"""
        changenode = ctx.node()
        rev = ctx.rev()

        if self.ui.quiet:
            self.ui.write("%s\n" % scmutil.formatchangeid(ctx), label="log.node")
            return

        columns = self._columns
        if changenode:
            self.ui.write(
                columns["changeset"] % scmutil.formatchangeid(ctx),
                label=_changesetlabels(ctx),
            )

        # branches are shown first before any other names due to backwards
        # compatibility
        branch = ctx.branch()
        # don't show the default branch name
        if branch != "default":
            self.ui.write(columns["branch"] % branch, label="log.branch")

        for nsname, ns in pycompat.iteritems(self.repo.names):
            # branches has special logic already handled above, so here we just
            # skip it
            if nsname == "branches":
                continue
            # we will use the templatename as the color name since those two
            # should be the same
            for name in ns.names(self.repo, changenode):
                self.ui.write(ns.logfmt % name, label="log.%s" % ns.colorname)
        if self.ui.debugflag:
            self.ui.write(columns["phase"] % ctx.phasestr(), label="log.phase")

        if self.ui.debugflag and rev is not None:
            mnode = ctx.manifestnode()
            self.ui.write(
                columns["manifest"] % hex(mnode), label="ui.debug log.manifest"
            )
        self.ui.write(columns["user"] % ctx.user(), label="log.user")
        self.ui.write(columns["date"] % util.datestr(ctx.date()), label="log.date")

        self._exthook(ctx)

        if self.ui.debugflag:
            files = ctx.p1().status(ctx)[:3]
            for key, value in zip(["files", "files+", "files-"], files):
                if value:
                    self.ui.write(
                        columns[key] % " ".join(value), label="ui.debug log.files"
                    )
        elif self.ui.verbose and ctx.files():
            self.ui.write(
                columns["files"] % " ".join(ctx.files()), label="ui.note log.files"
            )
        if copies and self.ui.verbose:
            copies = ["%s (%s)" % c for c in copies]
            self.ui.write(
                columns["copies"] % " ".join(copies), label="ui.note log.copies"
            )

        extra = ctx.extra()
        if extra and self.ui.debugflag:
            for key, value in sorted(extra.items()):
                self.ui.write(
                    columns["extra"] % (key, util.escapestr(value)),
                    label="ui.debug log.extra",
                )

        description = ctx.description().strip()
        if description:
            if self.ui.verbose:
                self.ui.write(_("description:\n"), label="ui.note log.description")
                self.ui.write(description, label="ui.note log.description")
                self.ui.write("\n\n")
            else:
                self.ui.write(
                    columns["summary"] % description.splitlines()[0],
                    label="log.summary",
                )
        self.ui.write("\n")

        self.showpatch(ctx, matchfn, hunksfilterfn=hunksfilterfn)

    def _exthook(self, ctx):
        """empty method used by extension as a hook point"""

    def showpatch(self, ctx, matchfn, hunksfilterfn=None):
        if not matchfn:
            matchfn = self.matchfn
        if matchfn:
            stat = self.diffopts.get("stat")
            diff = self.diffopts.get("patch")
            diffopts = patch.diffallopts(self.ui, self.diffopts)
            prevctx = ctx.p1()
            if stat:
                diffordiffstat(
                    self.ui,
                    self.repo,
                    diffopts,
                    prevctx,
                    ctx,
                    match=matchfn,
                    stat=True,
                    hunksfilterfn=hunksfilterfn,
                )
            if diff:
                if stat:
                    self.ui.write("\n")
                diffordiffstat(
                    self.ui,
                    self.repo,
                    diffopts,
                    prevctx,
                    ctx,
                    match=matchfn,
                    stat=False,
                    hunksfilterfn=hunksfilterfn,
                )
            self.ui.write("\n")


class jsonchangeset(changeset_printer):
    """format changeset information."""

    def __init__(self, ui, repo, matchfn, diffopts, buffered):
        changeset_printer.__init__(self, ui, repo, matchfn, diffopts, buffered)
        self.cache = {}
        self._first = True

    def close(self):
        if not self._first:
            self.ui.write("\n]\n")
        else:
            self.ui.write("[]\n")

    def _show(self, ctx, copies, matchfn, hunksfilterfn, props):
        """show a single changeset or file revision"""
        rev = ctx.rev()
        if rev is None:
            jrev = jnode = "null"
        else:
            jrev = "%d" % scmutil.revf64encode(rev)
            jnode = '"%s"' % hex(ctx.node())
        j = lambda v: json.dumps(v, paranoid=False)

        if self._first:
            self.ui.write("[\n {")
            self._first = False
        else:
            self.ui.write(",\n {")

        if self.ui.quiet:
            self.ui.write(_x('\n  "rev": %s') % jrev)
            self.ui.write(_x(',\n  "node": %s') % jnode)
            self.ui.write("\n }")
            return

        self.ui.write(_x('\n  "rev": %s') % jrev)
        self.ui.write(_x(',\n  "node": %s') % jnode)
        self.ui.write(_x(',\n  "branch": %s') % j(ctx.branch()))
        self.ui.write(_x(',\n  "phase": "%s"') % ctx.phasestr())
        self.ui.write(_x(',\n  "user": %s') % j(ctx.user()))
        self.ui.write(_x(',\n  "date": [%d, %d]') % ctx.date())
        self.ui.write(_x(',\n  "desc": %s') % j(ctx.description()))

        self.ui.write(
            _x(',\n  "bookmarks": [%s]')
            % ", ".join("%s" % j(b) for b in ctx.bookmarks())
        )
        self.ui.write(
            _x(',\n  "parents": [%s]')
            % ", ".join('"%s"' % c.hex() for c in ctx.parents())
        )

        if self.ui.debugflag:
            if rev is None:
                jmanifestnode = "null"
            else:
                jmanifestnode = '"%s"' % hex(ctx.manifestnode())
            self.ui.write(_x(',\n  "manifest": %s') % jmanifestnode)

            self.ui.write(
                _x(',\n  "extra": {%s}')
                % ", ".join("%s: %s" % (j(k), j(v)) for k, v in ctx.extra().items())
            )

            files = ctx.p1().status(ctx)
            self.ui.write(
                _x(',\n  "modified": [%s]') % ", ".join("%s" % j(f) for f in files[0])
            )
            self.ui.write(
                _x(',\n  "added": [%s]') % ", ".join("%s" % j(f) for f in files[1])
            )
            self.ui.write(
                _x(',\n  "removed": [%s]') % ", ".join("%s" % j(f) for f in files[2])
            )

        elif self.ui.verbose:
            self.ui.write(
                _x(',\n  "files": [%s]') % ", ".join("%s" % j(f) for f in ctx.files())
            )

            if copies:
                self.ui.write(
                    _x(',\n  "copies": {%s}')
                    % ", ".join("%s: %s" % (j(k), j(v)) for k, v in copies)
                )

        matchfn = self.matchfn
        if matchfn:
            stat = self.diffopts.get("stat")
            diff = self.diffopts.get("patch")
            diffopts = patch.difffeatureopts(self.ui, self.diffopts, git=True)
            prevctx = ctx.p1()
            if stat:
                self.ui.pushbuffer()
                diffordiffstat(
                    self.ui, self.repo, diffopts, prevctx, ctx, match=matchfn, stat=True
                )
                self.ui.write(
                    _x(',\n  "diffstat": %s') % json.dumps(self.ui.popbuffer())
                )
            if diff:
                self.ui.pushbuffer()
                diffordiffstat(
                    self.ui,
                    self.repo,
                    diffopts,
                    prevctx,
                    ctx,
                    match=matchfn,
                    stat=False,
                )
                # Don't use the j() helper because it expects utf8 strings.
                diff = encoding.jsonescape(self.ui.popbufferbytes())
                self.ui.writebytes(b',\n  "diff": "%s"' % diff)

        self.ui.write("\n }")


class changeset_templater(changeset_printer):
    """format changeset information.

    Note: there are a variety of convenience functions to build a
    changeset_templater for common cases. See functions such as:
    makelogtemplater, show_changeset, buildcommittemplate, or other
    functions that use changesest_templater.
    """

    # Arguments before "buffered" used to be positional. Consider not
    # adding/removing arguments before "buffered" to not break callers.
    def __init__(self, ui, repo, tmplspec, matchfn=None, diffopts=None, buffered=False):
        diffopts = diffopts or {}

        changeset_printer.__init__(self, ui, repo, matchfn, diffopts, buffered)
        self.t = formatter.loadtemplater(ui, tmplspec, cache=templatekw.defaulttempl)
        self._counter = itertools.count()
        self.cache = {}

        self._tref = tmplspec.ref
        self._parts = {
            "header": "",
            "footer": "",
            tmplspec.ref: tmplspec.ref,
            "docheader": "",
            "docfooter": "",
            "separator": "",
        }
        if tmplspec.mapfile:
            # find correct templates for current mode, for backward
            # compatibility with 'log -v/-q/--debug' using a mapfile
            tmplmodes = [
                (True, ""),
                (self.ui.verbose, "_verbose"),
                (self.ui.quiet, "_quiet"),
                (self.ui.debugflag, "_debug"),
            ]
            for mode, postfix in tmplmodes:
                for t in self._parts:
                    cur = t + postfix
                    if mode and cur in self.t:
                        self._parts[t] = cur
        else:
            partnames = [p for p in self._parts.keys() if p != tmplspec.ref]
            m = formatter.templatepartsmap(tmplspec, self.t, partnames)
            self._parts.update(m)

        if self._parts["docheader"]:
            self.ui.write(templater.stringify(self.t(self._parts["docheader"])))

    def close(self):
        if self._parts["docfooter"]:
            if not self.footer:
                self.footer = ""
            self.footer += templater.stringify(self.t(self._parts["docfooter"]))
        return super(changeset_templater, self).close()

    def _show(self, ctx, copies, matchfn, hunksfilterfn, props):
        """show a single changeset or file revision"""
        props = props.copy()
        props.update(templatekw.keywords)
        props["templ"] = self.t
        props["ctx"] = ctx
        props["repo"] = self.repo
        props["ui"] = self.repo.ui
        props["index"] = index = next(self._counter)
        props["revcache"] = {"copies": copies}
        props["cache"] = self.cache
        props = props

        # write separator, which wouldn't work well with the header part below
        # since there's inherently a conflict between header (across items) and
        # separator (per item)
        if self._parts["separator"] and index > 0:
            self.ui.write(templater.stringify(self.t(self._parts["separator"])))

        # write header
        if self._parts["header"]:
            h = templater.stringify(self.t(self._parts["header"], **props))
            if self.buffered:
                self.header[ctx.rev()] = h
            else:
                if self.lastheader != h:
                    self.lastheader = h
                    self.ui.write(h)

        # write changeset metadata, then patch if requested
        key = self._parts[self._tref]
        self.ui.writebytes(templater.byteify(self.t(key, **props)))
        self.showpatch(ctx, matchfn, hunksfilterfn=hunksfilterfn)

        if self._parts["footer"]:
            if not self.footer:
                self.footer = templater.stringify(
                    self.t(self._parts["footer"], **props)
                )


def logtemplatespec(tmpl, mapfile):
    if mapfile:
        return formatter.templatespec("changeset", tmpl, mapfile)
    else:
        return formatter.templatespec("", tmpl, None)


def _lookuplogtemplate(ui, tmpl, style):
    """Find the template matching the given template spec or style

    See formatter.lookuptemplate() for details.
    """

    # ui settings
    if not tmpl and not style:  # template are stronger than style
        tmpl = ui.config("ui", "logtemplate")
        if tmpl:
            return logtemplatespec(templater.unquotestring(tmpl), None)
        else:
            style = util.expandpath(ui.config("ui", "style"))

    if not tmpl and style:
        mapfile = style
        if not os.path.split(mapfile)[0]:
            mapname = templater.templatepath(
                "map-cmdline." + mapfile
            ) or templater.templatepath(mapfile)
            if mapname:
                mapfile = mapname
        return logtemplatespec(None, mapfile)

    if not tmpl:
        return logtemplatespec(None, None)

    return formatter.lookuptemplate(ui, "changeset", tmpl)


def makelogtemplater(ui, repo, tmpl, buffered=False):
    """Create a changeset_templater from a literal template 'tmpl'
    byte-string."""
    spec = logtemplatespec(tmpl, None)
    return changeset_templater(ui, repo, spec, buffered=buffered)


def show_changeset(ui, repo, opts, buffered=False):
    """show one changeset using template or regular display.

    Display format will be the first non-empty hit of:
    1. option 'template'
    2. option 'style'
    3. [ui] setting 'logtemplate'
    4. [ui] setting 'style'
    If all of these values are either the unset or the empty string,
    regular display via changeset_printer() is done.
    """
    # options
    match = None
    if opts.get("patch") or opts.get("stat"):
        match = scmutil.matchall(repo)

    if opts.get("template") == "json":
        return jsonchangeset(ui, repo, match, opts, buffered)

    spec = _lookuplogtemplate(ui, opts.get("template"), opts.get("style"))

    if not spec.ref and not spec.tmpl and not spec.mapfile:
        return changeset_printer(ui, repo, match, opts, buffered)

    return changeset_templater(ui, repo, spec, match, opts, buffered)


def showmarker(fm, marker, index=None):
    """utility function to display obsolescence marker in a readable way

    To be used by debug function."""
    if index is not None:
        fm.write("index", "%i ", index)
    fm.write("prednode", "%s ", hex(marker.prednode()))
    succs = marker.succnodes()
    fm.condwrite(
        succs, "succnodes", "%s ", fm.formatlist(list(map(hex, succs)), name="node")
    )
    fm.write("flag", "%X ", marker.flags())
    parents = marker.parentnodes()
    if parents is not None:
        fm.write(
            "parentnodes",
            "{%s} ",
            fm.formatlist(list(map(hex, parents)), name="node", sep=", "),
        )
    fm.write("date", "(%s) ", fm.formatdate(marker.date()))
    meta = marker.metadata().copy()
    meta.pop("date", None)
    fm.write("metadata", "{%s}", fm.formatdict(meta, fmt="%r: %r", sep=", "))
    fm.plain("\n")


def finddate(ui, repo, date):
    """Find the tipmost changeset that matches the given date spec"""

    df = util.matchdate(date)
    m = scmutil.matchall(repo)
    results = {}

    def prep(ctx, fns):
        d = ctx.date()
        if df(d[0]):
            results[ctx.node()] = d

    for ctx in walkchangerevs(repo, m, {"rev": None}, prep):
        node = ctx.node()
        if node in results:
            ui.status(
                _("found revision %s from %s\n")
                % (hex(node), util.datestr(results[node]))
            )
            return node

    raise error.Abort(_("revision matching date not found"))


def increasingwindows(windowsize=8, sizelimit=512):
    while True:
        yield windowsize
        if windowsize < sizelimit:
            windowsize *= 2


class FileWalkError(Exception):
    pass


def walkfilerevs(repo, match, follow, revs, fncache):
    """Walks the file history for the matched files.

    Returns the changeset revs that are involved in the file history.

    Throws FileWalkError if the file history can't be walked using
    filelogs alone.
    """
    wanted = set()
    copies = []
    minrev, maxrev = min(revs), max(revs)

    def filerevgen(filelog, last):
        """
        Only files, no patterns.  Check the history of each file.

        Examines filelog entries within minrev, maxrev linkrev range
        Returns an iterator yielding (linkrev, parentlinkrevs, copied)
        tuples in backwards order
        """
        cl_count = len(repo)
        revs = []
        for j in range(0, last + 1):
            linkrev = filelog.linkrev(j)
            if linkrev < minrev:
                continue
            # only yield rev for which we have the changelog, it can
            # happen while doing "hg log" during a pull or commit
            if linkrev >= cl_count:
                break

            parentlinkrevs = []
            for p in filelog.parentrevs(j):
                if p != nullrev:
                    parentlinkrevs.append(filelog.linkrev(p))
            n = filelog.node(j)
            revs.append((linkrev, parentlinkrevs, follow and filelog.renamed(n)))

        return reversed(revs)

    def iterfiles():
        pctx = repo["."]
        for filename in match.files():
            if follow:
                if filename not in pctx:
                    raise error.Abort(
                        _("cannot follow file not in parent " 'revision: "%s"')
                        % filename
                    )
                yield filename, pctx[filename].filenode()
            else:
                yield filename, None
        for filename_node in copies:
            yield filename_node

    for file_, node in iterfiles():
        filelog = repo.file(file_)
        if not len(filelog):
            if node is None:
                # A zero count may be a directory or deleted file, so
                # try to find matching entries on the slow path.
                if follow:
                    raise error.Abort(_('cannot follow nonexistent file: "%s"') % file_)
                raise FileWalkError("Cannot walk via filelog")
            else:
                continue

        if node is None:
            last = len(filelog) - 1
        else:
            last = filelog.rev(node)

        # keep track of all ancestors of the file
        ancestors = {filelog.linkrev(last)}

        # iterate from latest to oldest revision
        for rev, flparentlinkrevs, copied in filerevgen(filelog, last):
            if not follow:
                if rev > maxrev:
                    continue
            else:
                # Note that last might not be the first interesting
                # rev to us:
                # if the file has been changed after maxrev, we'll
                # have linkrev(last) > maxrev, and we still need
                # to explore the file graph
                if rev not in ancestors:
                    continue
                # XXX insert 1327 fix here
                if flparentlinkrevs:
                    ancestors.update(flparentlinkrevs)

            fncache.setdefault(rev, []).append(file_)
            wanted.add(rev)
            if copied:
                copies.append(copied)

    return wanted


class _followfilter(object):
    def __init__(self, repo, onlyfirst=False):
        self.repo = repo
        self.startrev = nullrev
        self.roots = set()
        self.onlyfirst = onlyfirst

    def match(self, rev):
        def realparents(rev):
            if self.onlyfirst:
                return self.repo.changelog.parentrevs(rev)[0:1]
            else:
                return filter(
                    lambda x: x != nullrev, self.repo.changelog.parentrevs(rev)
                )

        if self.startrev == nullrev:
            self.startrev = rev
            return True

        if rev > self.startrev:
            # forward: all descendants
            if not self.roots:
                self.roots.add(self.startrev)
            for parent in realparents(rev):
                if parent in self.roots:
                    self.roots.add(rev)
                    return True
        else:
            # backwards: all parents
            if not self.roots:
                self.roots.update(realparents(self.startrev))
            if rev in self.roots:
                self.roots.remove(rev)
                self.roots.update(realparents(rev))
                return True

        return False


def walkchangerevs(repo, match, opts, prepare):
    """Iterate over files and the revs in which they changed.

    Callers most commonly need to iterate backwards over the history
    in which they are interested. Doing so has awful (quadratic-looking)
    performance, so we use iterators in a "windowed" way.

    We walk a window of revisions in the desired order.  Within the
    window, we first walk forwards to gather data, then in the desired
    order (usually backwards) to display it.

    This function returns an iterator yielding contexts. Before
    yielding each context, the iterator will first call the prepare
    function on each context in the window in forward order."""

    follow = opts.get("follow") or opts.get("follow_first")
    revs = _logrevs(repo, opts)
    if not revs:
        return []
    wanted = set()
    slowpath = match.anypats() or (
        (match.isexact() or match.prefix()) and opts.get("removed")
    )
    fncache = {}
    change = repo.changectx

    # First step is to fill wanted, the set of revisions that we want to yield.
    # When it does not induce extra cost, we also fill fncache for revisions in
    # wanted: a cache of filenames that were changed (ctx.files()) and that
    # match the file filtering conditions.

    if match.always():
        # No files, no patterns.  Display all revs.
        wanted = revs
    elif not slowpath:
        # We only have to read through the filelog to find wanted revisions

        try:
            wanted = walkfilerevs(repo, match, follow, revs, fncache)
        except FileWalkError:
            slowpath = True

            # We decided to fall back to the slowpath because at least one
            # of the paths was not a file. Check to see if at least one of them
            # existed in history, otherwise simply return
            for path in match.files():
                if path == "." or path in repo.store:
                    break
            else:
                return []

    if slowpath:
        # We have to read the changelog to match filenames against
        # changed files

        if follow:
            raise error.Abort(
                _("can only follow copies/renames for explicit " "filenames")
            )

        # The slow path checks files modified in every changeset.
        # This is really slow on large repos, so compute the set lazily.
        class lazywantedset(object):
            def __init__(self):
                self.set = set()
                self.revs = set(revs)

            # No need to worry about locality here because it will be accessed
            # in the same order as the increasing window below.
            def __contains__(self, value):
                if value in self.set:
                    return True
                elif not value in self.revs:
                    return False
                else:
                    self.revs.discard(value)
                    ctx = change(value)
                    matches = filter(match, ctx.files())
                    if matches:
                        fncache[value] = matches
                        self.set.add(value)
                        return True
                    return False

            def discard(self, value):
                self.revs.discard(value)
                self.set.discard(value)

        wanted = lazywantedset()

    # it might be worthwhile to do this in the iterator if the rev range
    # is descending and the prune args are all within that range
    for rev in opts.get("prune", ()):
        rev = repo[rev].rev()
        ff = _followfilter(repo)
        stop = min(revs[0], revs[-1])
        for x in range(rev, stop - 1, -1):
            if ff.match(x):
                wanted = wanted - [x]

    # Now that wanted is correctly initialized, we can iterate over the
    # revision range, yielding only revisions in wanted.
    def iterate():
        if follow and match.always():
            ff = _followfilter(repo, onlyfirst=opts.get("follow_first"))

            def want(rev):
                return ff.match(rev) and rev in wanted

        else:

            def want(rev):
                return rev in wanted

        it = iter(revs)
        stopiteration = False
        for windowsize in increasingwindows():
            nrevs = []
            for i in range(windowsize):
                rev = next(it, None)
                if rev is None:
                    stopiteration = True
                    break
                elif want(rev):
                    nrevs.append(rev)
            for rev in sorted(nrevs):
                fns = fncache.get(rev)
                ctx = change(rev)
                if not fns:

                    def fns_generator():
                        for f in ctx.files():
                            if match(f):
                                yield f

                    fns = fns_generator()
                prepare(ctx, fns)
            for rev in nrevs:
                yield change(rev)

            if stopiteration:
                break

    return iterate()


def _makefollowlogfilematcher(repo, paths, followfirst, pctx):
    # When displaying a revision with --patch --follow FILE, we have
    # to know which file of the revision must be diffed. With
    # --follow, we want the names of the ancestors of FILE in the
    # revision, stored in "fcache". "fcache" is populated by
    # reproducing the graph traversal already done by --follow revset
    # and relating revs to file names (which is not "correct" but
    # good enough).
    fcache = {}
    fcacheready = [False]

    dirs = []
    files = []

    for path in paths:
        if path not in pctx:
            dirs.append(path)
        else:
            files.append(path)

    # When directories are passed in, walking the ancestors graph can be
    # extremely expensive, let's not attempt to do it and instead just match
    # all the files under the given directories.
    if not dirs:
        dirmatcher = None
    else:
        dirmatcher = matchmod.match(
            repo.root, repo.getcwd(), patterns=["path:%s" % path for path in dirs]
        )

    def populatefiles():
        # Walk the ancestors graph for all the files passed in.
        for fn in files:
            fctx = pctx[fn]
            fcache.setdefault(fctx.introrev(), set()).add(fctx.path())
            for c in fctx.ancestors(followfirst=followfirst):
                fcache.setdefault(c.rev(), set()).add(c.path())

    def filematcher(rev):
        if not fcacheready[0]:
            populatefiles()
            fcacheready[0] = True

        fileset = fcache.get(rev, [])
        if not fileset:
            filematcher = None
        else:
            filematcher = scmutil.matchfiles(repo, fcache.get(rev, []))

        return matchmod.union([dirmatcher, filematcher], repo.root, repo.getcwd())

    return filematcher


def _makenofollowlogfilematcher(repo, pats, opts):
    """hook for extensions to override the filematcher for non-follow cases"""
    return None


def _usepathhistory(repo):
    """whether to use the PathHistory API for log history"""
    # Git repo does not have filelog, linkrev. Must use PathHistory.
    if git.isgitformat(repo):
        return True
    return repo.ui.configbool("experimental", "pathhistory")


def _makelogrevset(repo, pats, opts, revs):
    """Return (expr, filematcher) where expr is a revset string built
    from log options and file patterns or None. If --stat or --patch
    are not passed filematcher is None. Otherwise it is a callable
    taking a revision number and returning a match objects filtering
    the files to be detailed when displaying the revision.
    """
    opt2revset = {
        "no_merges": ("not merge()", None),
        "only_merges": ("merge()", None),
        "_ancestors": ("ancestors(%(val)s)", None),
        "_fancestors": ("_firstancestors(%(val)s)", None),
        "_descendants": ("descendants(%(val)s)", None),
        "_fdescendants": ("_firstdescendants(%(val)s)", None),
        "_matchfiles": ("_matchfiles(%(val)s)", None),
        "date": ("date(%(val)r)", None),
        "branch": ("branch(%(val)r)", " or "),
        "_patslog": ("filelog(%(val)r)", " or "),
        "_patsfollow": ("follow(%(val)r)", " or "),
        "_patsfollowfirst": ("_followfirst(%(val)r)", " or "),
        "_pathhistory": ("_pathhistory(%(val)s)", " or "),
        "keyword": ("keyword(%(val)r)", " or "),
        "prune": ("not (%(val)r or ancestors(%(val)r))", " and "),
        "user": ("user(%(val)r)", " or "),
    }

    opts = dict(opts)
    # follow or not follow?
    follow = opts.get("follow") or opts.get("follow_first")
    usepathhistory = _usepathhistory(repo)
    if opts.get("follow_first"):
        followfirst = 1
    else:
        followfirst = 0
    # --follow with FILE behavior depends on revs...
    it = iter(revs)
    startrev = next(it)
    followdescendants = startrev < next(it, startrev)

    # branch and only_branch are really aliases and must be handled at
    # the same time
    opts["branch"] = opts.get("branch", []) + opts.get("only_branch", [])
    opts["branch"] = [repo.lookupbranch(b) for b in opts["branch"]]
    # pats/include/exclude are passed to match.match() directly in
    # _matchfiles() revset but walkchangerevs() builds its matcher with
    # scmutil.match(). The difference is input pats are globbed on
    # platforms without shell expansion (windows).
    wctx = repo[None]
    match, pats = scmutil.matchandpats(wctx, pats, opts)
    slowpath = match.anypats() or (
        (match.isexact() or match.prefix())
        and opts.get("removed")
        and not usepathhistory
    )
    # pathhistory can deal with directories and removed files.
    if not slowpath and not usepathhistory:
        for f in match.files():
            if follow and f not in wctx:
                # If the file exists, it may be a directory. The "follow"
                # revset can handle directories fine. So no need to use
                # the slow path.
                if os.path.exists(repo.wjoin(f)):
                    continue
                else:
                    raise error.Abort(
                        _("cannot follow file not in parent " 'revision: "%s"') % f
                    )
            filelog = repo.file(f)
            if not filelog:
                # A zero count may be a directory or deleted file, so
                # try to find matching entries on the slow path.
                if follow:
                    raise error.Abort(_('cannot follow nonexistent file: "%s"') % f)
                slowpath = True

        # We decided to fall back to the slowpath because at least one
        # of the paths was not a file. Check to see if at least one of them
        # existed in history - in that case, we'll continue down the
        # slowpath; otherwise, we can turn off the slowpath
        if slowpath:
            for path in match.files():
                if path == "." or path in repo.store:
                    break
            else:
                slowpath = False

    fpats = ("_patsfollow", "_patsfollowfirst")
    fnopats = (("_ancestors", "_fancestors"), ("_descendants", "_fdescendants"))
    if slowpath:
        # See walkchangerevs() slow path.
        #
        # pats/include/exclude cannot be represented as separate
        # revset expressions as their filtering logic applies at file
        # level. For instance "-I a -X a" matches a revision touching
        # "a" and "b" while "file(a) and not file(b)" does
        # not. Besides, filesets are evaluated against the working
        # directory.
        matchargs = ["r:", "d:relpath"]
        for p in pats:
            matchargs.append("p:" + p)
        for p in opts.get("include", []):
            matchargs.append("i:" + p)
        for p in opts.get("exclude", []):
            matchargs.append("x:" + p)
        matchargs = ",".join(("%r" % p) for p in matchargs)
        opts["_matchfiles"] = matchargs
        if follow:
            opts[fnopats[0][followfirst]] = "."
    else:
        # pathhistory: force "follow" if "pats" is given.
        if usepathhistory:
            if pats:
                paths = list(match.files())
                if followfirst:
                    phrevs = "_firstancestors(rev(%d))" % startrev
                else:
                    phrevs = "ancestors(rev(%d))" % startrev
                phfiles = ",".join(map(repr, paths))
                opts["_pathhistory"] = "%s,%s" % (phrevs, phfiles)
        if follow:
            if pats:
                # pathhistory handled this above
                if not usepathhistory:
                    # follow() revset interprets its file argument as a
                    # manifest entry, so use match.files(), not pats.
                    opts[fpats[followfirst]] = list(match.files())
            else:
                op = fnopats[followdescendants][followfirst]
                opts[op] = "rev(%d)" % startrev
        else:
            # avoid using filelog() (_patslog) if pathhistory is used
            if not usepathhistory:
                opts["_patslog"] = list(pats)

    filematcher = None
    if opts.get("patch") or opts.get("stat"):
        # When following files, track renames via a special matcher.
        # If we're forced to take the slowpath it means we're following
        # at least one pattern/directory, so don't bother with rename tracking.
        #
        # If path history is used, avoid using filelog and linkrev.
        if follow and not match.always() and not slowpath and not usepathhistory:
            # _makefollowlogfilematcher expects its files argument to be
            # relative to the repo root, so use match.files(), not pats.
            filematcher = _makefollowlogfilematcher(
                repo, match.files(), followfirst, repo[startrev]
            )
        else:
            filematcher = _makenofollowlogfilematcher(repo, pats, opts)
            if filematcher is None:
                filematcher = lambda rev: match

    expr = []
    for op, val in sorted(pycompat.iteritems(opts)):
        if not val:
            continue
        if op not in opt2revset:
            continue
        revop, andor = opt2revset[op]
        if "%(val)" not in revop:
            expr.append(revop)
        else:
            if not isinstance(val, list):
                e = revop % {"val": val}
            else:
                e = "(" + andor.join((revop % {"val": v}) for v in val) + ")"
            expr.append(e)

    if expr:
        expr = "(" + " and ".join(expr) + ")"
        tracing.debug("log revset: %s\n" % expr, target="log::makelogrevset")
    else:
        expr = None
    return expr, filematcher


def _logrevs(repo, opts):
    # Default --rev value depends on --follow but --follow behavior
    # depends on revisions resolved from --rev...
    follow = opts.get("follow") or opts.get("follow_first")
    if opts.get("rev"):
        revs = scmutil.revrange(repo, opts["rev"])
    elif follow and repo.dirstate.p1() == nullid:
        revs = smartset.baseset(repo=repo)
    elif follow:
        revs = repo.revs("reverse(:.)")
    else:
        revs = repo.revs("all()")
        revs.reverse()
    return revs


def getgraphlogrevs(repo, pats, opts):
    """Return (revs, expr, filematcher) where revs is an iterable of
    revision numbers, expr is a revset string built from log options
    and file patterns or None, and used to filter 'revs'. If --stat or
    --patch are not passed filematcher is None. Otherwise it is a
    callable taking a revision number and returning a match objects
    filtering the files to be detailed when displaying the revision.
    """
    limit = loglimit(opts)
    revs = _logrevs(repo, opts)
    if not revs:
        return smartset.baseset(repo=repo), None, None
    expr, filematcher = _makelogrevset(repo, pats, opts, revs)
    if expr:
        if opts.get("rev"):
            revs = repo.revs(expr, subset=revs)
        else:
            # revs is likely huge. "x & y" is more efficient if "x" is small.
            # "x & y" respects "x"'s order. Once rewritten to "y & x", the
            # order is decided by "y". Fortunately we know the order of "x" is
            # always "reverse" in this case. So just do a reverse sort.
            revs = repo.revs(expr) & revs
            revs.sort(reverse=True)
    if opts.get("rev"):
        # User-specified revs might be unsorted, but don't sort before
        # _makelogrevset because it might depend on the order of revs
        if not (revs.isdescending() or revs.istopo()):
            revs.sort(reverse=True)
    if limit is not None:
        limitedrevs = []
        for idx, rev in enumerate(revs):
            if idx >= limit:
                break
            limitedrevs.append(rev)
        revs = smartset.baseset(limitedrevs, repo=repo)

    return revs, expr, filematcher


def getlogrevs(repo, pats, opts):
    """Return (revs, expr, filematcher) where revs is an iterable of
    revision numbers, expr is a revset string built from log options
    and file patterns or None, and used to filter 'revs'. If --stat or
    --patch are not passed filematcher is None. Otherwise it is a
    callable taking a revision number and returning a match objects
    filtering the files to be detailed when displaying the revision.
    """
    limit = loglimit(opts)
    revs = _logrevs(repo, opts)
    if not revs:
        return smartset.baseset([], repo=repo), None, None
    expr, filematcher = _makelogrevset(repo, pats, opts, revs)
    if expr:
        if opts.get("rev"):
            revs = repo.revs(expr, subset=revs)
        else:
            # revs is likely huge. "x & y" is more efficient if "x" is small.
            # "x & y" respects "x"'s order. Once rewritten to "y & x", the
            # order is decided by "y". Fortunately we know the order of "x" is
            # always "reverse" in this case. So just do a reverse sort.
            revs = repo.revs(expr) & revs
            revs.sort(reverse=True)
    if limit is not None:
        limitedrevs = []
        for idx, r in enumerate(revs):
            if limit <= idx:
                break
            limitedrevs.append(r)
        revs = smartset.baseset(limitedrevs, repo=repo)

    return revs, expr, filematcher


def _parselinerangelogopt(repo, opts):
    """Parse --line-range log option and return a list of tuples (filename,
    (fromline, toline)).
    """
    linerangebyfname = []
    for pat in opts.get("line_range", []):
        try:
            pat, linerange = pat.rsplit(",", 1)
        except ValueError:
            raise error.Abort(_("malformatted line-range pattern %s") % pat)
        try:
            fromline, toline = list(map(int, linerange.split(":")))
        except ValueError:
            raise error.Abort(_("invalid line range for %s") % pat)
        msg = _("line range pattern '%s' must match exactly one file") % pat
        fname = scmutil.parsefollowlinespattern(repo, None, pat, msg)
        linerangebyfname.append((fname, util.processlinerange(fromline, toline)))
    return linerangebyfname


def getloglinerangerevs(repo, userrevs, opts):
    """Return (revs, filematcher, hunksfilter).

    "revs" are revisions obtained by processing "line-range" log options and
    walking block ancestors of each specified file/line-range.

    "filematcher(rev) -> match" is a factory function returning a match object
    for a given revision for file patterns specified in --line-range option.
    If neither --stat nor --patch options are passed, "filematcher" is None.

    "hunksfilter(rev) -> filterfn(fctx, hunks)" is a factory function
    returning a hunks filtering function.
    If neither --stat nor --patch options are passed, "filterhunks" is None.
    """
    wctx = repo[None]

    # Two-levels map of "rev -> file ctx -> [line range]".
    linerangesbyrev = {}
    for fname, (fromline, toline) in _parselinerangelogopt(repo, opts):
        if fname not in wctx:
            raise error.Abort(
                _("cannot follow file not in parent " 'revision: "%s"') % fname
            )
        fctx = wctx.filectx(fname)
        for fctx, linerange in dagop.blockancestors(fctx, fromline, toline):
            rev = fctx.introrev()
            if rev not in userrevs:
                continue
            linerangesbyrev.setdefault(rev, {}).setdefault(fctx.path(), []).append(
                linerange
            )

    filematcher = None
    hunksfilter = None
    if opts.get("patch") or opts.get("stat"):

        def nofilterhunksfn(fctx, hunks):
            return hunks

        def hunksfilter(rev):
            fctxlineranges = linerangesbyrev.get(rev)
            if fctxlineranges is None:
                return nofilterhunksfn

            def filterfn(fctx, hunks):
                lineranges = fctxlineranges.get(fctx.path())
                if lineranges is not None:
                    for hr, lines in hunks:
                        if hr is None:  # binary
                            yield hr, lines
                            continue
                        if any(mdiff.hunkinrange(hr[2:], lr) for lr in lineranges):
                            yield hr, lines
                else:
                    for hunk in hunks:
                        yield hunk

            return filterfn

        def filematcher(rev):
            files = list(linerangesbyrev.get(rev, []))
            return scmutil.matchfiles(repo, files)

    revs = sorted(linerangesbyrev, reverse=True)

    return smartset.baseset(revs, repo=repo), filematcher, hunksfilter


def _graphnodeformatter(ui, displayer):
    spec = ui.config("ui", "graphnodetemplate")
    if not spec:
        return templatekw.showgraphnode  # fast path for "{graphnode}"

    spec = templater.unquotestring(spec)
    templ = formatter.maketemplater(ui, spec)
    cache = {}
    if isinstance(displayer, changeset_templater):
        cache = displayer.cache  # reuse cache of slow templates
    props = templatekw.keywords.copy()
    props["templ"] = templ
    props["cache"] = cache

    def formatnode(repo, ctx):
        props["ctx"] = ctx
        props["repo"] = repo
        props["ui"] = repo.ui
        props["revcache"] = {}
        return templ.render(props)

    return formatnode


def displaygraph(
    ui,
    repo,
    dag,
    displayer,
    getrenamed=None,
    filematcher=None,
    props=None,
    reserved=None,
    out=None,
):
    props = props or {}
    formatnode = _graphnodeformatter(ui, displayer)
    if ui.plain("graph"):
        renderername = "ascii"
    else:
        renderername = ui.config("experimental", "graph.renderer")
    if renderername == "lines":
        # Find which renderer can render to the current output encoding.  If
        # none are supported we will fall back to the ASCII renderer.
        for chars, candidate in [
            (renderdag.linescurvedchars, "lines-curved"),
            (renderdag.linessquarechars, "lines-square"),
        ]:
            try:
                chars.encode(encoding.outputencoding or encoding.encoding, "strict")
                renderername = candidate
                break
            except Exception:
                continue
    renderers = {
        "ascii": renderdag.ascii,
        "ascii-large": renderdag.asciilarge,
        "lines-curved": renderdag.linescurved,
        "lines-square": renderdag.linessquare,
        "lines-dec": renderdag.linesdec,
    }
    renderer = renderers.get(renderername, renderdag.ascii)
    minheight = 1 if ui.configbool("experimental", "graphshorten") else 2
    minheight = ui.configint("experimental", "graph.min-row-height", minheight)
    renderer = renderer(minheight)

    if reserved:
        for rev in reserved:
            renderer.reserve(rev)

    for (rev, _type, ctx, parents) in dag:
        char = formatnode(repo, ctx)
        copies = None
        if getrenamed and ctx.rev():
            copies = []
            for fn in ctx.files():
                rename = getrenamed(fn, ctx.rev())
                if rename:
                    copies.append((fn, rename[0]))
        revmatchfn = None
        if filematcher is not None:
            revmatchfn = filematcher(ctx.rev())
        width = renderer.width(rev, parents)
        displayer.show(
            ctx, copies=copies, matchfn=revmatchfn, _graphwidth=width, **props
        )
        # The Rust graph renderer works with unicode.
        msg = "".join(
            ensureunicode(encoding.unifromlocal(s), errors="replace")
            for s in displayer.hunk.pop(rev)
        )
        nextrow = renderer.nextrow(rev, parents, char, msg)
        if out is not None:
            out(nextrow)
        else:
            ui.write(encoding.unitolocal(nextrow))
        displayer.flush(ctx)

    displayer.close()


def graphlog(ui, repo, pats, opts):
    # Parameters are identical to log command ones
    revs, expr, filematcher = getgraphlogrevs(repo, pats, opts)
    template = opts.get("template") or ""
    revdag = graphmod.dagwalker(repo, revs, template)

    getrenamed = None
    if opts.get("copies"):
        endrev = None
        if opts.get("rev"):
            endrev = scmutil.revrange(repo, opts.get("rev")).max() + 1
        getrenamed = templatekw.getrenamedfn(repo, endrev=endrev)

    ui.pager("log")
    displayer = show_changeset(ui, repo, opts, buffered=True)
    displaygraph(ui, repo, revdag, displayer, getrenamed, filematcher)


def checkunsupportedgraphflags(pats, opts):
    for op in ["newest_first"]:
        if op in opts and opts[op]:
            raise error.Abort(
                _("-G/--graph option is incompatible with --%s") % op.replace("_", "-")
            )


def graphrevs(repo, nodes, opts):
    limit = loglimit(opts)
    nodes.reverse()
    if limit is not None:
        nodes = nodes[:limit]
    return graphmod.nodes(repo, nodes)


def add(ui, repo, match, prefix, explicitonly, **opts):
    bad = []

    badfn = lambda x, y: bad.append(x) or match.bad(x, y)
    names = []
    wctx = repo[None]
    pctx = wctx.p1()
    cca = None
    abort, warn = scmutil.checkportabilityalert(ui)
    if abort or warn:
        cca = scmutil.casecollisionauditor(ui, abort, repo.dirstate)

    badmatch = matchmod.badmatch(match, badfn)

    # While we technically can't add certain files, and therefore may not need
    # them returned from status, we still want them returned from status so we
    # can report errors if the user tries to add something that already exists.
    status = repo.dirstate.status(badmatch, False, False, True)
    files = set(file for files in status for file in files)

    # Status might not have returned clean or ignored files, so let's add them
    # so we can add ignored files and warn them if they try to add an existing
    # file.
    ignored = repo.dirstate._ignore
    files.update(
        file
        for file in match.files()
        if file in pctx or (ignored(file) and repo.wvfs.isfileorlink(file))
    )

    for f in sorted(files):
        exact = match.exact(f)
        if exact or not explicitonly and f not in wctx and repo.wvfs.lexists(f):
            if cca:
                cca(f)
            names.append(f)
            if ui.verbose or not exact:
                ui.status(_("adding %s\n") % match.rel(f))

    if not opts.get(r"dry_run"):
        rejected = wctx.add(names, prefix)
        bad.extend(f for f in rejected if f in match.files())
    return bad


def addwebdirpath(repo, serverpath, webconf):
    webconf[serverpath] = repo.root
    repo.ui.debug("adding %s = %s\n" % (serverpath, repo.root))


def forget(ui, repo, match, prefix, explicitonly):
    bad = []
    badfn = lambda x, y: bad.append(x) or match.bad(x, y)
    wctx = repo[None]
    forgot = []

    s = repo.status(match=matchmod.badmatch(match, badfn), clean=True)
    forget = sorted(s.modified + s.added + s.deleted + s.clean)
    if explicitonly:
        forget = [f for f in forget if match.exact(f)]

    if not explicitonly:
        for f in match.files():
            if f not in repo.dirstate and not repo.wvfs.isdir(f):
                if f not in forgot:
                    if repo.wvfs.exists(f):
                        # Don't complain if the exact case match wasn't given.
                        # But don't do this until after checking 'forgot', so
                        # that subrepo files aren't normalized, and this op is
                        # purely from data cached by the status walk above.
                        if repo.dirstate.normalize(f) in repo.dirstate:
                            continue
                        ui.warn(
                            _("not removing %s: " "file is already untracked\n")
                            % match.rel(f)
                        )
                    bad.append(f)

    for f in forget:
        if ui.verbose or not match.exact(f):
            ui.status(_("removing %s\n") % match.rel(f))

    rejected = wctx.forget(forget, prefix)
    bad.extend(f for f in rejected if f in match.files())
    forgot.extend(f for f in forget if f not in rejected)
    return bad, forgot


def files(ui, ctx, m, fm, fmt):
    if (ctx.rev() is None) and (edenfs.requirement in ctx.repo().requirements):
        return eden_files(ui, ctx, m, fm, fmt)

    rev = ctx.rev()
    ret = 1
    ds = ctx.repo().dirstate

    for f in ctx.matches(m):
        if rev is None and ds[f] == "r":
            continue
        fm.startitem()
        if ui.verbose:
            fc = ctx[f]
            fm.write("size flags", "% 10d % 1s ", fc.size(), fc.flags())
        fm.data(abspath=f)
        fm.write("path", fmt, m.rel(f))
        ret = 0

    return ret


def eden_files(ui, ctx, m, fm, fmt):
    # The default files() function code looks up the dirstate entry for ever
    # single matched file.  This is unnecessary in most cases, and will trigger
    # a lot of thrift calls to Eden.  We have augmented the Eden dirstate with
    # a function that can return only non-removed files without requiring
    # looking up every single match.
    ret = 1
    ds = ctx.repo().dirstate
    for f in sorted(ds.non_removed_matches(m)):
        fm.startitem()
        if ui.verbose:
            fc = ctx[f]
            fm.write("size flags", "% 10d % 1s ", fc.size(), fc.flags())
        fm.data(abspath=f)
        fm.write("path", fmt, m.rel(f))
        ret = 0

    return ret


def remove(ui, repo, m, prefix, after, force, warnings=None):
    ret = 0
    clean = force or not after
    s = repo.status(match=m, clean=clean)
    modified, added, deleted, clean = s[0], s[1], s[3], s[6]

    wctx = repo[None]

    if warnings is None:
        warnings = []
        warn = True
    else:
        warn = False

    # warn about failure to delete explicit files/dirs
    deleteddirs = util.dirs(deleted)
    files = m.files()
    with progress.bar(ui, _("deleting"), _("files"), len(files)) as prog:
        for f in files:
            prog.value += 1
            isdir = f in deleteddirs or wctx.hasdir(f)
            if f in repo.dirstate or isdir or f == "":
                continue

            if repo.wvfs.exists(f):
                if repo.wvfs.isdir(f):
                    warnings.append(_("not removing %s: no tracked files\n") % m.rel(f))
                else:
                    warnings.append(
                        _("not removing %s: file is untracked\n") % m.rel(f)
                    )
            # missing files will generate a warning elsewhere
            ret = 1

    if force:
        list = modified + deleted + clean + added
    elif after:
        list = deleted
        # For performance, "remaining" only lists "exact" matches.
        # In theory it should also list "clean" files but that's too expensive
        # for a large repo.
        remaining = set(files) - set(deleted) - set(s.removed)
        for f in sorted(remaining):
            if repo.wvfs.exists(f) and not repo.wvfs.isdir(f):
                warnings.append(_("not removing %s: file still exists\n") % m.rel(f))
                ret = 1
    else:
        list = deleted + clean
        total = len(modified) + len(added)
        with progress.bar(ui, _("skipping"), _("files"), total) as prog:
            for f in modified:
                prog.value += 1
                warnings.append(
                    _(
                        "not removing %s: file is modified (use -f"
                        " to force removal)\n"
                    )
                    % m.rel(f)
                )
                ret = 1
            for f in added:
                prog.value += 1
                warnings.append(
                    _(
                        "not removing %s: file has been marked for"
                        " add (use '@prog@ forget' to undo add)\n"
                    )
                    % m.rel(f)
                )
                ret = 1

    list = sorted(list)
    with progress.bar(ui, _("deleting"), _("files"), len(list)) as prog:
        for i, f in enumerate(list, 1):
            if ui.verbose or not m.exact(f):
                prog.value = i
                ui.status(_("removing %s\n") % m.rel(f))

    with repo.wlock():
        if not after:
            for f in list:
                if f in added:
                    continue  # we never unlink added files on remove
                repo.wvfs.unlinkpath(f, ignoremissing=True)
        repo[None].forget(list)

    if warn:
        for warning in warnings:
            ui.warn(warning)

    return ret


def cat(ui, repo, ctx, matcher, basefm, fntemplate, prefix, **opts):
    err = 1

    def write(path):
        filename = None
        if fntemplate:
            filename = makefilename(
                repo, fntemplate, ctx.node(), pathname=os.path.join(prefix, path)
            )
            # attempt to create the directory if it does not already exist
            try:
                os.makedirs(os.path.dirname(filename))
            except OSError:
                pass
        with formatter.maybereopen(basefm, filename, opts) as fm:
            data = ctx[path].data()
            if opts.get("decode"):
                data = repo.wwritedata(path, data)
            fm.startitem()
            fm.writebytes("data", b"%s", data)
            fm.data(abspath=path, path=matcher.rel(path))

    # Automation often uses hg cat on single files, so special case it
    # for performance to avoid the cost of parsing the manifest.
    if len(matcher.files()) == 1 and not matcher.anypats():
        file = matcher.files()[0]
        mfl = repo.manifestlog
        mfnode = ctx.manifestnode()
        try:
            if mfnode and mfl[mfnode].find(file)[0]:
                write(file)
                return 0
        except KeyError:
            pass

    for abs in ctx.walk(matcher):
        write(abs)
        err = 0

    return err


def commit(ui, repo, commitfunc, pats, opts):
    """commit the specified files or all outstanding changes"""
    date = opts.get("date")
    if date:
        opts["date"] = util.parsedate(date)
    message = logmessage(repo, opts)
    matcher = scmutil.match(repo[None], pats, opts)

    dsguard = None
    # extract addremove carefully -- this function can be called from a command
    # that doesn't support addremove
    if opts.get("addremove"):
        dsguard = dirstateguard.dirstateguard(repo, "commit")
    with dsguard or util.nullcontextmanager():
        if dsguard:
            if scmutil.addremove(repo, matcher, "", opts) != 0:
                raise error.Abort(
                    _("failed to mark all new/missing files as added/removed")
                )

        return commitfunc(ui, repo, message, matcher, opts)


def samefile(f, ctx1, ctx2, m1=None, m2=None):
    if m1 is None:
        m1 = ctx1.manifest()
    if m2 is None:
        m2 = ctx2.manifest()
    if f in m1:
        a = ctx1.filectx(f)
        if f in m2:
            b = ctx2.filectx(f)
            return not a.cmp(b) and a.flags() == b.flags()
        else:
            return False
    else:
        return f not in m2


def amend(ui, repo, old, extra, pats, opts):
    # avoid cycle context -> subrepo -> cmdutil
    from . import context

    ui.note(_("amending changeset %s\n") % old)
    base = old.p1()

    with repo.wlock(), repo.lock(), repo.transaction("amend"):
        # Participating changesets:
        #
        # wctx     o - workingctx that contains changes from working copy
        #          |   to go into amending commit
        #          |
        # old      o - changeset to amend
        #          |
        # base     o - first parent of the changeset to amend
        wctx = repo[None]

        # Copy to avoid mutating input
        extra = extra.copy()
        # Update extra dict from amended commit (e.g. to preserve graft
        # source)
        extra.update(old.extra())

        # Also update it from the from the wctx
        extra.update(wctx.extra())

        user = opts.get("user") or old.user()
        date = opts.get("date") or old.date()

        # Parse the date to allow comparison between date and old.date()
        date = util.parsedate(date)

        if len(old.parents()) > 1:
            # ctx.files() isn't reliable for merges, so fall back to the
            # slower repo.status() method
            files = set([fn for st in repo.status(base, old)[:3] for fn in st])
        else:
            files = set(old.files())

        # add/remove the files to the working copy if the "addremove" option
        # was specified.
        matcher = scmutil.match(wctx, pats, opts)
        if opts.get("addremove") and scmutil.addremove(repo, matcher, "", opts):
            raise error.Abort(
                _("failed to mark all new/missing files as added/removed")
            )

        # avoid cycle (TODO: should be removed in default branch)
        from . import merge as mergemod

        ms = mergemod.mergestate.read(repo)
        mergeutil.checkunresolved(ms)

        status = repo.status(match=matcher)
        filestoamend = set(status.modified + status.added + status.removed)

        changes = len(filestoamend) > 0
        if changes:
            # Recompute copies (avoid recording a -> b -> a)
            copied = copies.pathcopies(base, wctx, matcher)
            if old.p2:
                copied.update(copies.pathcopies(old.p2(), wctx, matcher))

            # Prune files which were reverted by the updates: if old
            # introduced file X and the file was renamed in the working
            # copy, then those two files are the same and
            # we can discard X from our list of files. Likewise if X
            # was removed, it's no longer relevant. If X is missing (aka
            # deleted), old X must be preserved.
            with perftrace.trace("Prune files reverted by amend"):
                statusmanifest = wctx.buildstatusmanifest(status)
                for f in filestoamend:
                    if (
                        not samefile(f, wctx, base, m1=statusmanifest)
                        or f in status.deleted
                    ):
                        files.add(f)
                    else:
                        files.discard(f)
                files = list(files)

            def filectxfn(repo, ctx_, path):
                try:
                    # If the file being considered is not amongst the files
                    # to be amended, we should return the file context from the
                    # old changeset. This avoids issues when only some files in
                    # the working copy are being amended but there are also
                    # changes to other files from the old changeset.
                    if path not in filestoamend:
                        return old.filectx(path)

                    # Return None for removed files.
                    if path in status.removed:
                        return None

                    fctx = wctx[path]
                except KeyError:
                    return None
                else:
                    c = copied.get(path, False)
                    return context.overlayfilectx(fctx, ctx=ctx_, copied=c)

        else:
            ui.note(_("copying changeset %s to %s\n") % (old, base))

            # Use version of files as in the old cset
            def filectxfn(repo, ctx_, path):
                try:
                    return old.filectx(path)
                except KeyError:
                    return None

        # See if we got a message from -m or -l, if not, open the editor with
        # the message of the changeset to amend.
        message = logmessage(repo, opts)

        editform = mergeeditform(old, "commit.amend")
        editor = getcommiteditor(editform=editform, **opts)

        if not message:
            editor = getcommiteditor(edit=True, editform=editform)
            message = old.description()

        pureextra = extra.copy()
        extra["amend_source"] = old.hex()
        mutinfo = mutation.record(repo, extra, [old.node()], "amend")

        loginfo = {
            "checkoutidentifier": repo.dirstate.checkoutidentifier,
            "predecessors": old.hex(),
            "mutation": "amend",
        }

        new = context.memctx(
            repo,
            parents=[base, old.p2()],
            text=message,
            files=files,
            filectxfn=filectxfn,
            user=user,
            date=date,
            extra=extra,
            editor=editor,
            loginfo=loginfo,
            mutinfo=mutinfo,
        )

        newdesc = changelog.stripdesc(new.description())
        if (
            (not changes)
            and newdesc == old.description()
            and user == old.user()
            and date == old.date()
            and pureextra == old.extra()
        ):
            # nothing changed. continuing here would create a new node
            # anyway because of the mutation or amend_source data.
            #
            # This not what we expect from amend.
            return old.node()

        if opts.get("secret"):
            commitphase = "secret"
        else:
            commitphase = old.phase()
        overrides = {("phases", "new-commit"): commitphase}
        with ui.configoverride(overrides, "amend"):
            newid = repo.commitctx(new)

        # Reroute the working copy parent to the new changeset
        repo.setparents(newid, nullid)
        mapping = {old.node(): (newid,)}
        obsmetadata = None
        if opts.get("note"):
            obsmetadata = {"note": opts["note"]}
        scmutil.cleanupnodes(repo, mapping, "amend", metadata=obsmetadata)

        # Fixing the dirstate because localrepo.commitctx does not update
        # it. This is rather convenient because we did not need to update
        # the dirstate for all the files in the new commit which commitctx
        # could have done if it updated the dirstate. Now, we can
        # selectively update the dirstate only for the amended files.
        dirstate = repo.dirstate

        # Update the state of the files which were added and
        # and modified in the amend to "normal" in the dirstate.
        normalfiles = set(status.modified + status.added) & filestoamend
        for f in normalfiles:
            dirstate.normal(f)

        # Update the state of files which were removed in the amend
        # to "removed" in the dirstate.
        removedfiles = set(status.removed) & filestoamend
        for f in removedfiles:
            dirstate.untrack(f)

    return newid


def commiteditor(repo, ctx, editform=""):
    if ctx.description():
        return ctx.description()
    return commitforceeditor(
        repo, ctx, editform=editform, unchangedmessagedetection=True
    )


def commitforceeditor(
    repo,
    ctx,
    finishdesc=None,
    extramsg=None,
    editform="",
    unchangedmessagedetection=False,
):
    if not extramsg:
        extramsg = _("Leave message empty to abort commit.")

    forms = [e for e in editform.split(".") if e]
    forms.insert(0, "changeset")
    templatetext = None
    while forms:
        ref = ".".join(forms)
        if repo.ui.config("committemplate", ref):
            templatetext = committext = buildcommittemplate(repo, ctx, extramsg, ref)
            break
        forms.pop()
    else:
        committext = buildcommittext(repo, ctx, extramsg)

    # run editor in the repository root
    olddir = pycompat.getcwd()
    os.chdir(repo.root)

    # make in-memory changes visible to external process
    tr = repo.currenttransaction()
    repo.dirstate.write(tr)
    if tr and tr.writepending():
        pending = repo.root
        sharedpending = repo.sharedroot
    else:
        pending = sharedpending = None

    editortext = repo.ui.edit(
        committext,
        ctx.user(),
        ctx.extra(),
        editform=editform,
        pending=pending,
        sharedpending=sharedpending,
        repopath=repo.path,
        action="commit",
    )
    text = editortext

    # strip away anything below this special string (used for editors that want
    # to display the diff)
    stripbelow = re.search(_linebelow, text, flags=re.MULTILINE)
    if stripbelow:
        text = text[: stripbelow.start()]

    all_prefixes = "|".join(
        ident.cliname().upper() for ident in bindings.identity.all()
    )
    text = re.sub(f"(?m)^({all_prefixes}):.*(\n|$)", "", text)
    os.chdir(olddir)

    if finishdesc:
        text = finishdesc(text)
    if not text.strip():
        raise error.Abort(_("empty commit message"))
    if unchangedmessagedetection and editortext == templatetext:
        raise error.Abort(_("commit message unchanged"))

    return text


def buildcommittemplate(repo, ctx, extramsg, ref):
    ui = repo.ui
    spec = formatter.templatespec(ref, None, None)
    t = changeset_templater(ui, repo, spec, None, {}, False)
    t.t.cache.update(
        (k, templater.unquotestring(v))
        for k, v in repo.ui.configitems("committemplate")
    )

    if not extramsg:
        extramsg = ""  # ensure that extramsg is string

    ui.pushbuffer()
    t.show(ctx, extramsg=extramsg)
    return pycompat.decodeutf8(ui.popbufferbytes(), errors="replace")


def hgprefix(msg):
    return "\n".join([f"{identity.tmplprefix()}: {a}" for a in msg.split("\n") if a])


def buildcommittext(repo, ctx, extramsg):
    edittext = []
    modified, added, removed = ctx.modified(), ctx.added(), ctx.removed()
    if ctx.description():
        edittext.append(ctx.description())
    edittext.append("")
    edittext.append("")  # Empty line between message and comments.
    edittext.append(
        hgprefix(
            _(
                "Enter commit message."
                f"  Lines beginning with '{identity.tmplprefix()}:' are removed."
            )
        )
    )
    edittext.append(hgprefix(extramsg))
    edittext.append(f"{identity.tmplprefix()}: --")
    edittext.append(hgprefix(_("user: %s") % ctx.user()))
    if ctx.p2():
        edittext.append(hgprefix(_("branch merge")))
    if ctx.branch():
        edittext.append(hgprefix(_("branch '%s'") % ctx.branch()))
    if bookmarks.isactivewdirparent(repo):
        edittext.append(hgprefix(_("bookmark '%s'") % repo._activebookmark))
    edittext.extend([hgprefix(_("added %s") % f) for f in added])
    edittext.extend([hgprefix(_("changed %s") % f) for f in modified])
    edittext.extend([hgprefix(_("removed %s") % f) for f in removed])
    if not added and not modified and not removed:
        edittext.append(hgprefix(_("no files changed")))
    edittext.append("")

    return "\n".join(edittext)


def commitstatus(repo, node, branch, opts=None):
    if opts is None:
        opts = {}
    ctx = repo[node]
    parents = ctx.parents()

    if not opts.get("close_branch"):
        for r in parents:
            if r.closesbranch() and r.branch() == branch:
                repo.ui.status(_("reopening closed branch head %d\n") % r)

    if repo.ui.debugflag:
        repo.ui.write(_("committed %s\n") % (ctx.hex()))
    elif repo.ui.verbose:
        repo.ui.write(_("committed %s\n") % (ctx))


def postcommitstatus(repo, pats, opts):
    return repo.status(match=scmutil.match(repo[None], pats, opts))


def revert(ui, repo, ctx, parents, *pats, **opts):
    parent, p2 = parents
    node = ctx.node()

    mf = ctx.manifest()
    if node == p2:
        parent = p2

    # need all matching names in dirstate and manifest of target rev,
    # so have to walk both. do not print errors if files exist in one
    # but not other. in both cases, filesets should be evaluated against
    # workingctx to get consistent result (issue4497). this means 'set:**'
    # cannot be used to select missing files from target rev.

    # `names` is a mapping for all elements in working copy and target revision
    # The mapping is in the form:
    #   <asb path in repo> -> (<path from CWD>, <exactly specified by matcher?>)
    names = {}

    with repo.wlock():
        ## filling of the `names` mapping
        # walk dirstate to fill `names`

        interactive = opts.get("interactive", False)
        wctx = repo[None]
        m = scmutil.match(wctx, pats, opts)

        # we'll need this later
        badfiles = set()

        def badfn(path, msg):
            # We only report errors about paths that do not exist in the original
            # node.
            #
            # For other errors we normally will successfully revert the file anyway.
            # This includes situations where the file was replaced by an unsupported
            # file type (e.g., a FIFO, socket, or device node.
            if path not in ctx:
                badfiles.add(path)
                msg = _("no such file in rev %s") % short(node)
                ui.warn("%s: %s\n" % (m.rel(path), msg))

        changes = repo.status(node1=node, match=matchmod.badmatch(m, badfn))
        for kind in changes:
            for abs in kind:
                names[abs] = m.rel(abs), m.exact(abs)

        # Look for exact filename matches that were not returned in the results.
        # These will not be returned if they are clean, but we want to include them
        # to report them as not needing changes.
        for abs in m.files():
            if abs in names or abs in badfiles:
                continue
            # Check to see if this looks like a file or directory.
            # We don't need to report directories in the clean list.
            try:
                st = repo.wvfs.lstat(abs)
                if stat.S_ISDIR(st.st_mode):
                    continue
            except OSError:
                # This is can occur if the file was locally removed and is
                # untracked, and did not exist in the node we are reverting from,
                # but does exist in the current commit.
                # Continue on and report this file as clean.
                pass

            names[abs] = m.rel(abs), m.exact(abs)
            if abs in wctx:
                changes.clean.append(abs)
            else:
                # We don't really know if this file is unknown or ignored, but
                # fortunately this does not matter.  Revert treats unknown and
                # ignored exactly files the same.
                changes.unknown.append(abs)

        modified = set(changes.modified)
        added = set(changes.added)
        removed = set(changes.removed)
        _deleted = set(changes.deleted)
        unknown = set(changes.unknown)
        unknown.update(changes.ignored)
        clean = set(changes.clean)
        modadded = set()

        # We need to account for the state of the file in the dirstate,
        # even when we revert against something else than parent. This will
        # slightly alter the behavior of revert (doing back up or not, delete
        # or just forget etc).
        if parent == node:
            dsmodified = modified
            dsadded = added
            dsremoved = removed
            # store all local modifications, useful later for rename detection
            localchanges = dsmodified | dsadded
            modified, added, removed = set(), set(), set()
        else:
            exactfilesmatch = scmutil.matchfiles(repo, names)
            changes = repo.status(node1=parent, match=exactfilesmatch)
            dsmodified = set(changes.modified)
            dsadded = set(changes.added)
            dsremoved = set(changes.removed)
            # store all local modifications, useful later for rename detection
            localchanges = dsmodified | dsadded

            # only take into account for removes between wc and target
            clean |= dsremoved - removed
            dsremoved &= removed
            # distinct between dirstate remove and other
            removed -= dsremoved

            modadded = added & dsmodified
            added -= modadded

            # tell newly modified apart.
            dsmodified &= modified
            dsmodified |= modified & dsadded  # dirstate added may need backup
            modified -= dsmodified

            # We need to wait for some post-processing to update this set
            # before making the distinction. The dirstate will be used for
            # that purpose.
            dsadded = added

        # in case of merge, files that are actually added can be reported as
        # modified, we need to post process the result
        if p2 != nullid:
            mergeadd = set(dsmodified)
            for path in dsmodified:
                if path in mf:
                    mergeadd.remove(path)
            dsadded |= mergeadd
            dsmodified -= mergeadd

        # if f is a rename, update `names` to also revert the source
        cwd = repo.getcwd()
        for f in localchanges:
            src = repo.dirstate.copied(f)
            # XXX should we check for rename down to target node?
            if src and src not in names and repo.dirstate[src] == "r":
                dsremoved.add(src)
                names[src] = (repo.pathto(src, cwd), True)

        # determine the exact nature of the deleted changesets
        deladded = set(_deleted)
        for path in _deleted:
            if path in mf:
                deladded.remove(path)
        deleted = _deleted - deladded

        # distinguish between file to forget and the other
        added = set()
        for abs in dsadded:
            if repo.dirstate[abs] != "a":
                added.add(abs)
        dsadded -= added

        for abs in deladded:
            if repo.dirstate[abs] == "a":
                dsadded.add(abs)
        deladded -= dsadded

        # For files marked as removed, we check if an unknown file is present at
        # the same path. If a such file exists it may need to be backed up.
        # Making the distinction at this stage helps have simpler backup
        # logic.
        removunk = set()
        for abs in removed:
            target = repo.wjoin(abs)
            if os.path.lexists(target):
                removunk.add(abs)
        removed -= removunk

        dsremovunk = set()
        for abs in dsremoved:
            target = repo.wjoin(abs)
            if os.path.lexists(target):
                dsremovunk.add(abs)
        dsremoved -= dsremovunk

        # action to be actually performed by revert
        # (<list of file>, message>) tuple
        actions = {
            "revert": ([], _("reverting %s\n")),
            "add": ([], _("adding %s\n")),
            "remove": ([], _("removing %s\n")),
            "drop": ([], _("removing %s\n")),
            "forget": ([], _("forgetting %s\n")),
            "undelete": ([], _("undeleting %s\n")),
            "noop": (None, _("no changes needed to %s\n")),
            "unknown": (None, _("file not managed: %s\n")),
        }

        # "constant" that convey the backup strategy.
        # All set to `discard` if `no-backup` is set do avoid checking
        # no_backup lower in the code.
        # These values are ordered for comparison purposes
        backupinteractive = 3  # do backup if interactively modified
        backup = 2  # unconditionally do backup
        check = 1  # check if the existing file differs from target
        discard = 0  # never do backup
        if opts.get("no_backup"):
            backupinteractive = backup = check = discard
        if interactive:
            dsmodifiedbackup = backupinteractive
        else:
            dsmodifiedbackup = backup
        tobackup = set()

        backupanddel = actions["remove"]
        if not opts.get("no_backup"):
            backupanddel = actions["drop"]

        disptable = (
            # dispatch table:
            #   file state
            #   action
            #   make backup
            ## Sets that results that will change file on disk
            # Modified compared to target, no local change
            (modified, actions["revert"], discard),
            # Modified compared to target, but local file is deleted
            (deleted, actions["revert"], discard),
            # Modified compared to target, local change
            (dsmodified, actions["revert"], dsmodifiedbackup),
            # Added since target
            (added, actions["remove"], discard),
            # Added in working directory
            (dsadded, actions["forget"], discard),
            # Added since target, have local modification
            (modadded, backupanddel, backup),
            # Added since target but file is missing in working directory
            (deladded, actions["drop"], discard),
            # Removed since  target, before working copy parent
            (removed, actions["add"], discard),
            # Same as `removed` but an unknown file exists at the same path
            (removunk, actions["add"], check),
            # Removed since targe, marked as such in working copy parent
            (dsremoved, actions["undelete"], discard),
            # Same as `dsremoved` but an unknown file exists at the same path
            (dsremovunk, actions["undelete"], check),
            ## the following sets does not result in any file changes
            # File with no modification
            (clean, actions["noop"], discard),
            # Existing file, not tracked anywhere
            (unknown, actions["unknown"], discard),
        )

        quiet = ui.quiet
        for abs, (rel, exact) in sorted(names.items()):
            # target file to be touch on disk (relative to cwd)
            target = repo.wjoin(abs)
            # search the entry in the dispatch table.
            # if the file is in any of these sets, it was touched in the working
            # directory parent and we are sure it needs to be reverted.
            for table, (xlist, msg), dobackup in disptable:
                if abs not in table:
                    continue
                if xlist is not None:
                    xlist.append(abs)
                    if dobackup:
                        # If in interactive mode, don't automatically create
                        # .orig files (issue4793)
                        if dobackup == backupinteractive:
                            tobackup.add(abs)
                        elif (backup <= dobackup or wctx[abs].cmp(ctx[abs])) and wctx[
                            abs
                        ].flags() != "m":
                            bakname = scmutil.origpath(ui, repo, rel)
                            ui.note(
                                _("saving current version of %s as %s\n")
                                % (rel, bakname)
                            )
                            if not opts.get("dry_run"):
                                # Don't backup symlinks, since they can
                                # interfere with future backup paths that
                                # overlap with the symlink path (like
                                # accidentally trying to move something
                                # into the symlink).
                                if not os.path.islink(target):
                                    if interactive:
                                        util.copyfile(target, bakname)
                                    else:
                                        util.rename(target, bakname)
                    if ui.verbose or not exact:
                        if not isinstance(msg, str):
                            msg = msg(abs)
                        ui.status(msg % rel)
                elif exact and not quiet:
                    ui.warn(msg % rel)
                break

        if not opts.get("dry_run"):
            needdata = ("revert", "add", "undelete")
            _revertprefetch(repo, ctx, *[actions[name][0] for name in needdata])
            _performrevert(
                repo,
                parents,
                ctx,
                actions,
                interactive,
                tobackup,
                forcecopytracing=opts.get("forcecopytracing"),
            )


def _revertprefetch(repo, ctx, *files):
    """Let extension changing the storage layer prefetch content"""


def _performrevert(
    repo,
    parents,
    ctx,
    actions,
    interactive=False,
    tobackup=None,
    forcecopytracing=False,
):
    """function that actually perform all the actions computed for revert

    This is an independent function to let extension to plug in and react to
    the imminent revert.

    Make sure you have the working directory locked when calling this function.
    """
    parent, p2 = parents
    node = ctx.node()
    excluded_files = []
    matcher_opts = {"exclude": excluded_files}
    wctx = repo[None]

    def checkout(f):
        fc = ctx[f]
        if fc.flags() == "m":
            # f is a submodule, need special path to change
            git.submodulecheckout(ctx, match=lambda p: p == f, force=True)
            return
        wctx[f].clearunknown()
        repo.wwrite(f, fc.data(), fc.flags())

    def doremove(f):
        try:
            repo.wvfs.unlinkpath(f)
        except OSError:
            pass
        repo.dirstate.remove(f)

    audit_path = pathutil.pathauditor(repo.root, cached=True)
    for f in actions["forget"][0]:
        if interactive:
            choice = repo.ui.promptchoice(
                _("forget added file %s (Yn)?$$ &Yes $$ &No") % f
            )
            if choice == 0:
                repo.dirstate.untrack(f)
            else:
                excluded_files.append(repo.wjoin(f))
        else:
            repo.dirstate.untrack(f)
    for f in actions["remove"][0]:
        audit_path(f)
        if interactive:
            choice = repo.ui.promptchoice(
                _("remove added file %s (Yn)?$$ &Yes $$ &No") % f
            )
            if choice == 0:
                doremove(f)
            else:
                excluded_files.append(repo.wjoin(f))
        else:
            doremove(f)
    for f in actions["drop"][0]:
        audit_path(f)
        repo.dirstate.remove(f)

    # By default, use "normallookup" so the file is marked as
    # "need content check" for the next "status" run. The only fast path is
    # when reverting to the non-merge working parent, where the file can be
    # marked as clean.
    normal = repo.dirstate.normallookup
    if node == parent and p2 == nullid:
        # We're reverting to our parent. If possible, we'd like status
        # to report the file as clean. We have to use normallookup for
        # merges to avoid losing information about merged/dirty files.
        normal = repo.dirstate.normal

    newlyaddedandmodifiedfiles = set()
    if interactive:
        # Prompt the user for changes to revert
        torevert = [repo.wjoin(f) for f in actions["revert"][0]]
        m = scmutil.match(ctx, torevert, matcher_opts)
        diffopts = patch.difffeatureopts(repo.ui, whitespace=True)
        diffopts.nodates = True
        diffopts.git = True
        operation = "discard"
        reversehunks = True
        if node != parent:
            operation = "apply"
            reversehunks = False
        if reversehunks:
            diff = patch.diff(repo, ctx, repo[None], m, opts=diffopts)
        else:
            diff = patch.diff(repo, repo[None], ctx, m, opts=diffopts)
        originalchunks = patch.parsepatch(diff)

        try:

            chunks, opts = recordfilter(repo.ui, originalchunks, operation=operation)
            if reversehunks:
                chunks = patch.reversehunks(chunks)

        except error.PatchError as err:
            raise error.Abort(_("error parsing patch: %s") % err)

        newlyaddedandmodifiedfiles = newandmodified(chunks, originalchunks)
        if tobackup is None:
            tobackup = set()
        # Apply changes
        fp = stringio()
        for c in chunks:
            # Create a backup file only if this hunk should be backed up
            if ishunk(c) and c.header.filename() in tobackup:
                abs = c.header.filename()
                target = repo.wjoin(abs)
                bakname = scmutil.origpath(repo.ui, repo, m.rel(abs))
                util.copyfile(target, bakname)
                tobackup.remove(abs)
            c.write(fp)
        dopatch = fp.tell()
        fp.seek(0)
        if dopatch:
            try:
                patch.internalpatch(repo.ui, repo, fp, 1, eolmode=None)
            except error.PatchError as err:
                raise error.Abort(str(err))
        del fp
    else:
        for f in actions["revert"][0]:
            checkout(f)
            normal(f)

    for f in actions["add"][0]:
        # Don't checkout modified files, they are already created by the diff
        if f not in newlyaddedandmodifiedfiles:
            checkout(f)
            repo.dirstate.add(f)

    for f in actions["undelete"][0]:
        checkout(f)
        normal(f)

    if forcecopytracing or repo.ui.config("experimental", "copytrace") != "off":
        # When reverting a change, always enable copy tracing so we don't
        # accidentally lose any data.
        with repo.ui.configoverride({("experimental", "copytrace"): "on"}):
            copied = copies.pathcopies(repo[parent], ctx)

        for f in actions["add"][0] + actions["undelete"][0] + actions["revert"][0]:
            if f in copied:
                repo.dirstate.copy(copied[f], f)


class command(registrar.command):
    """deprecated: used registrar.command instead"""

    def _doregister(self, func, name, *args, **kwargs):
        func._deprecatedregistrar = True  # flag for deprecwarn in extensions.py
        return super(command, self)._doregister(func, name, *args, **kwargs)


# a list of (ui, repo, otherpeer, opts, missing) functions called by
# commands.outgoing.  "missing" is "missing" of the result of
# "findcommonoutgoing()"
outgoinghooks = util.hooks()

# a list of (ui, repo) functions called by commands.summary
summaryhooks = util.hooks()

# a list of (ui, repo, opts, changes) functions called by commands.summary.
#
# functions should return tuple of booleans below, if 'changes' is None:
#  (whether-incomings-are-needed, whether-outgoings-are-needed)
#
# otherwise, 'changes' is a tuple of tuples below:
#  - (sourceurl, sourcebranch, sourcepeer, incoming)
#  - (desturl,   destbranch,   destpeer,   outgoing)
summaryremotehooks = util.hooks()

# A list of state files kept by multistep operations like graft.
# Since graft cannot be aborted, it is considered 'clearable' by update.
# note: bisect is intentionally excluded
# (state file, clearable, allowcommit, error, hint)
unfinishedstates = [
    (
        "graftstate",
        True,
        False,
        _("graft in progress"),
        _("use '@prog@ graft --continue' or '@prog@ graft --abort' to abort"),
    ),
    (
        "updatemergestate",
        True,
        True,
        _("update --merge in progress"),
        _("use '@prog@ goto --continue' to continue"),
    ),
    (
        "updatestate",
        True,
        False,
        _("last update was interrupted"),
        _(
            "use '@prog@ goto DESTINATION' to get a consistent checkout\n"
            "note: '@prog@ goto --continue' is supported in some cases, such as "
            "during clone, and will resume the checkout where it left off"
        ),
    ),
]


def checkunfinished(repo, commit=False):
    """Look for an unfinished multistep operation, like graft, and abort
    if found. It's probably good to check this right before
    bailifchanged().
    """
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if commit and allowcommit:
            continue
        if repo.localvfs.exists(f):
            raise error.Abort(msg, hint=hint)


def clearunfinished(repo):
    """Check for unfinished operations (as above), and clear the ones
    that are clearable.
    """
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if not clearable and repo.localvfs.exists(f):
            raise error.Abort(msg, hint=hint)
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if clearable and repo.localvfs.exists(f):
            util.unlink(repo.localvfs.join(f))


afterresolvedstates = [
    ("graftstate", _("@prog@ graft --continue")),
    ("updatemergestate", _("@prog@ goto --continue")),
]


def howtocontinue(repo):
    """Check for an unfinished operation and return the command to finish
    it.

    afterresolvedstates tuples define a .hg/{file} and the corresponding
    command needed to finish it.

    Returns a (msg, warning) tuple. 'msg' is a string and 'warning' is
    a boolean.
    """
    contmsg = _("continue: %s")
    for f, msg in afterresolvedstates:
        if repo.localvfs.exists(f):
            return contmsg % msg, True
    if repo[None].dirty(missing=True, merge=False, branch=False):
        return contmsg % _("@prog@ commit"), False
    return None, None


def checkafterresolved(repo):
    """Inform the user about the next action after completing hg resolve

    If there's a matching afterresolvedstates, howtocontinue will yield
    repo.ui.warn as the reporter.

    Otherwise, it will yield repo.ui.note.
    """
    msg, warning = howtocontinue(repo)
    if msg is not None:
        if warning:
            repo.ui.warn("%s\n" % msg)
        else:
            repo.ui.note("%s\n" % msg)


def wrongtooltocontinue(repo, task):
    """Raise an abort suggesting how to properly continue if there is an
    active task.

    Uses howtocontinue() to find the active task.

    If there's no task (repo.ui.note for 'hg commit'), it does not offer
    a hint.
    """
    after = howtocontinue(repo)
    hint = None
    if after[1]:
        hint = after[0]
    raise error.Abort(_("no %s in progress") % task, hint=hint)

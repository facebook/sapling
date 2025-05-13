# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# extdiff.py - external diff program support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""command to allow external programs to compare revisions

The extdiff Mercurial extension allows you to use external programs
to compare revisions, or revision with working directory. The external
diff programs are called with a configurable set of options and two
non-option arguments: paths to directories containing snapshots of
files to compare.

The extdiff extension also allows you to configure new diff commands, so
you do not need to type :prog:`extdiff -p kdiff3` always. ::

  [extdiff]
  # add new command that runs GNU diff(1) in 'context diff' mode
  cdiff = gdiff -Nprc5
  ## or the old way:
  #cmd.cdiff = gdiff
  #opts.cdiff = -Nprc5

  # add new command called meld, runs meld (no need to name twice).  If
  # the meld executable is not available, the meld tool in [merge-tools]
  # will be used, if available
  meld =

  # add new command called vimdiff, runs gvimdiff with DirDiff plugin
  # (see http://www.vim.org/scripts/script.php?script_id=102) Non
  # English user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
  # your .vimrc
  vimdiff = gvim -f "+next" \\
            "+execute 'DirDiff' fnameescape(argv(0)) fnameescape(argv(1))"

Tool arguments can include variables that are expanded at runtime::

  $parent1, $plabel1 - filename, descriptive label of first parent
  $child,   $clabel  - filename, descriptive label of child revision
  $parent2, $plabel2 - filename, descriptive label of second parent
  $root              - repository root
  $parent is an alias for $parent1.

The extdiff extension will look in your [diff-tools] and [merge-tools]
sections for diff tool arguments, when none are specified in [extdiff].

::

  [extdiff]
  kdiff3 =

  [diff-tools]
  kdiff3.diffargs=--L1 '$plabel1' --L2 '$clabel' $parent $child

You can use -I/-X and list of file or directory names like normal
:prog:`diff` command. The extdiff extension makes snapshots of only
needed files, so running the external diff program will actually be
pretty fast (at least faster than having to compare the entire tree).
"""

import os
import re
import shlex
import shutil
import tempfile

from sapling import archival, cmdutil, error, filemerge, registrar, scmutil, util
from sapling.i18n import _
from sapling.node import nullid, short

cmdtable = {}
command = registrar.command(cmdtable)


# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"


def snapshot(ui, repo, files, node, tmproot):
    """snapshot files as of some revision
    if not using snapshot, -I/-X does not work and recursive diff
    in tools like kdiff3 and meld displays too many files."""
    dirname = os.path.basename(repo.root)
    if dirname == "":
        dirname = "root"
    if node is not None:
        dirname = "%s.%s" % (dirname, short(node))
    base = os.path.join(tmproot, dirname)
    os.mkdir(base)
    fnsandstat = []

    if node is not None:
        ui.note(
            _("making snapshot of %d files from rev %s\n") % (len(files), short(node))
        )
    else:
        ui.note(
            _("making snapshot of %d files from working directory\n") % (len(files))
        )

    if files:
        repo.ui.setconfig("ui", "archivemeta", False)

        archival.archive(
            repo, base, node, "files", matchfn=scmutil.matchfiles(repo, files)
        )

        for fn in sorted(files):
            wfn = util.pconvert(fn)
            ui.note("  %s\n" % wfn)

            if node is None:
                dest = os.path.join(base, wfn)

                fnsandstat.append((dest, repo.wjoin(fn), util.lstat(dest)))
    return dirname, fnsandstat


def dodiff(ui, repo, cmdline, pats, opts):
    """Do the actual diff:

    - copy to a temp structure if diffing 2 internal revisions
    - copy to a temp structure if diffing working revision with
      another one and more than 1 file is changed
    - just invoke the diff for a single file in the working dir
    """

    revs = opts.get("rev")
    change = opts.get("change")
    do3way = "$parent2" in cmdline

    if revs and change:
        msg = _("cannot specify --rev and --change at the same time")
        raise error.Abort(msg)
    elif change:
        node2 = scmutil.revsingle(repo, change, None).node()
        node1a, node1b = repo.changelog.parents(node2)
    else:
        node1a, node2 = scmutil.revpair(repo, revs)
        if not revs:
            node1b = repo.dirstate.p2()
        else:
            node1b = nullid

    # Disable 3-way merge if there is only one parent
    if do3way:
        if node1b == nullid:
            do3way = False

    matcher = scmutil.match(repo[node2], pats, opts)

    if opts.get("patch"):
        if node2 is None:
            raise error.Abort(_("--patch requires two revisions"))
    else:
        mod_a, add_a, rem_a = list(map(set, repo.status(node1a, node2, matcher)[:3]))
        if do3way:
            mod_b, add_b, rem_b = list(
                map(set, repo.status(node1b, node2, matcher)[:3])
            )
        else:
            mod_b, add_b, rem_b = set(), set(), set()
        modadd = mod_a | add_a | mod_b | add_b
        common = modadd | rem_a | rem_b
        if not common:
            return 0

    tmproot = tempfile.mkdtemp(prefix="extdiff.")
    try:
        if not opts.get("patch"):
            # Always make a copy of node1a (and node1b, if applicable)
            dir1a_files = mod_a | rem_a | ((mod_b | add_b) - add_a)
            dir1a = snapshot(ui, repo, dir1a_files, node1a, tmproot)[0]
            rev1a = "@%d" % repo[node1a].rev()
            if do3way:
                dir1b_files = mod_b | rem_b | ((mod_a | add_a) - add_b)
                dir1b = snapshot(ui, repo, dir1b_files, node1b, tmproot)[0]
                rev1b = "@%d" % repo[node1b].rev()
            else:
                dir1b = None
                rev1b = ""

            fnsandstat = []

            # If node2 in not the wc or there is >1 change, copy it
            dir2root = ""
            rev2 = ""
            if node2:
                dir2 = snapshot(ui, repo, modadd, node2, tmproot)[0]
                rev2 = "@%d" % repo[node2].rev()
            elif len(common) > 1:
                # we only actually need to get the files to copy back to
                # the working dir in this case (because the other cases
                # are: diffing 2 revisions or single file -- in which case
                # the file is already directly passed to the diff tool).
                dir2, fnsandstat = snapshot(ui, repo, modadd, None, tmproot)
            else:
                # This lets the diff tool open the changed file directly
                dir2 = ""
                dir2root = repo.root

            label1a = rev1a
            label1b = rev1b
            label2 = rev2

            # If only one change, diff the files instead of the directories
            # Handle bogus modifies correctly by checking if the files exist
            if len(common) == 1:
                common_file = util.localpath(common.pop())
                dir1a = os.path.join(tmproot, dir1a, common_file)
                label1a = common_file + rev1a
                if not os.path.isfile(dir1a):
                    dir1a = os.devnull
                if do3way:
                    dir1b = os.path.join(tmproot, dir1b, common_file)
                    label1b = common_file + rev1b
                    if not os.path.isfile(dir1b):
                        dir1b = os.devnull
                dir2 = os.path.join(dir2root, dir2, common_file)
                label2 = common_file + rev2
        else:
            template = "hg-%h.patch"
            cmdutil.export(
                repo,
                [repo[node1a].rev(), repo[node2].rev()],
                fntemplate=repo.localvfs.reljoin(tmproot, template),
                match=matcher,
            )
            label1a = cmdutil.makefilename(repo, template, node1a)
            label2 = cmdutil.makefilename(repo, template, node2)
            dir1a = repo.localvfs.reljoin(tmproot, label1a)
            dir2 = repo.localvfs.reljoin(tmproot, label2)
            dir1b = None
            label1b = None
            fnsandstat = []

        # Function to quote file/dir names in the argument string.
        # When not operating in 3-way mode, an empty string is
        # returned for parent2
        replace = {
            "parent": dir1a,
            "parent1": dir1a,
            "parent2": dir1b,
            "plabel1": label1a,
            "plabel2": label1b,
            "clabel": label2,
            "child": dir2,
            "root": repo.root,
        }

        def quote(match):
            pre = match.group(2)
            key = match.group(3)
            if not do3way and key == "parent2":
                return pre
            return pre + util.shellquote(replace[key])

        # Match parent2 first, so 'parent1?' will match both parent1 and parent
        regex = (
            r"""(['"]?)([^\s'"$]*)"""
            r"\$(parent2|parent1?|child|plabel1|plabel2|clabel|root)\1"
        )
        if not do3way and not re.search(regex, cmdline):
            cmdline += " $parent1 $child"
        cmdline = re.sub(regex, quote, cmdline)

        ui.debug("running %r in %s\n" % (cmdline, tmproot))
        ui.system(cmdline, cwd=tmproot, blockedtag="extdiff")

        for copy_fn, working_fn, st in fnsandstat:
            cpstat = util.lstat(copy_fn)
            # Some tools copy the file and attributes, so mtime may not detect
            # all changes.  A size check will detect more cases, but not all.
            # The only certain way to detect every case is to diff all files,
            # which could be expensive.
            # copyfile() carries over the permission, so the mode check could
            # be in an 'elif' branch, but for the case where the file has
            # changed without affecting mtime or size.
            if (
                cpstat.st_mtime != st.st_mtime
                or cpstat.st_size != st.st_size
                or (cpstat.st_mode & 0o100) != (st.st_mode & 0o100)
            ):
                ui.debug(
                    "file changed while diffing. "
                    "Overwriting: %s (src: %s)\n" % (working_fn, copy_fn)
                )
                util.copyfile(copy_fn, working_fn)

        return 1
    finally:
        ui.note(_("cleaning up temp directory\n"))
        shutil.rmtree(tmproot)


extdiffopts = [
    ("o", "option", [], _("pass option to comparison program"), _("OPT")),
    ("r", "rev", [], _("revision"), _("REV")),
    ("c", "change", "", _("change made by revision"), _("REV")),
    ("", "patch", None, _("compare patches for two revisions")),
] + cmdutil.walkopts


@command(
    "extdiff|extd|extdi|extdif",
    [("p", "program", "", _("comparison program to run"), _("CMD"))] + extdiffopts,
    _("@prog@ extdiff [OPT]... [FILE]..."),
    inferrepo=True,
)
def extdiff(ui, repo, *pats, **opts):
    """use external program to diff repository (or selected files)

    Show differences between revisions for the specified files, using
    an external program. The default program used is diff, with
    default options "-Npru".

    To select a different program, use the -p/--program option. The
    program will be passed the names of two directories to compare. To
    pass additional options to the program, use -o/--option. These
    will be passed before the names of the directories to compare.

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent."""
    program = opts.get("program")
    option = opts.get("option")
    if not program:
        program = "diff"
        option = option or ["-Npru"]
    cmdline = " ".join(map(util.shellquote, [program] + option))
    return dodiff(ui, repo, cmdline, pats, opts)


class savedcmd:
    """use external program to diff repository (or selected files)

    Show differences between revisions for the specified files, using
    the following program::

        %(path)s

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent.
    """

    def __init__(self, path, cmdline):
        # We can't pass non-ASCII through docstrings (and path is
        # in an unknown encoding anyway)
        docpath = util.escapestr(path)
        self.__doc__ = self.__doc__ % {"path": util.uirepr(docpath)}
        self._cmdline = cmdline

    def __call__(self, ui, repo, *pats, **opts):
        options = " ".join(map(util.shellquote, opts["option"]))
        if options:
            options = " " + options
        return dodiff(ui, repo, self._cmdline + options, pats, opts)


def uisetup(ui):
    for cmd, path in ui.configitems("extdiff"):
        path = util.expandpath(path)
        if cmd.startswith("cmd."):
            cmd = cmd[4:]
            if not path:
                path = util.findexe(cmd)
                if path is None:
                    path = filemerge.findexternaltool(ui, cmd) or cmd
            diffopts = ui.config("extdiff", "opts." + cmd)
            cmdline = util.shellquote(path)
            if diffopts:
                cmdline += " " + diffopts
        elif cmd.startswith("opts."):
            continue
        else:
            if path:
                # case "cmd = path opts"
                cmdline = path
                diffopts = len(shlex.split(cmdline)) > 1
            else:
                # case "cmd ="
                path = util.findexe(cmd)
                if path is None:
                    path = filemerge.findexternaltool(ui, cmd) or cmd
                cmdline = util.shellquote(path)
                diffopts = False
        # look for diff arguments in [diff-tools] then [merge-tools]
        if not diffopts:
            args = ui.config("diff-tools", cmd + ".diffargs") or ui.config(
                "merge-tools", cmd + ".diffargs"
            )
            if args:
                cmdline += " " + args
        command(
            cmd,
            extdiffopts[:],
            _("@prog@ %s [OPTION]... [FILE]...") % cmd,
            inferrepo=True,
        )(savedcmd(path, cmdline))


# tell hggettext to extract docstrings from these functions:
i18nfunctions = [savedcmd]

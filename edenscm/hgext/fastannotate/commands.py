# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# commands: fastannotate commands

from __future__ import absolute_import

import os

from edenscm.mercurial import (
    commands,
    error,
    extensions,
    patch,
    progress,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from . import context as facontext, error as faerror, formatter as faformatter


cmdtable = {}
command = registrar.command(cmdtable)


def _matchpaths(repo, rev, pats, opts, aopts=facontext.defaultopts):
    """generate paths matching given patterns"""
    perfhack = repo.ui.configbool("fastannotate", "perfhack")

    # disable perfhack if:
    # a) any walkopt is used
    # b) if we treat pats as plain file names, some of them do not have
    #    corresponding linelog files
    if perfhack:
        # cwd related to reporoot
        reporoot = os.path.dirname(repo.path)
        reldir = os.path.relpath(pycompat.getcwd(), reporoot)
        if reldir == ".":
            reldir = ""
        if any(opts.get(o[1]) for o in commands.walkopts):  # a)
            perfhack = False
        else:  # b)
            relpats = [
                os.path.relpath(p, reporoot) if os.path.isabs(p) else p for p in pats
            ]
            # disable perfhack on '..' since it allows escaping from the repo
            if any(
                (
                    ".." in f
                    or not os.path.isfile(
                        facontext.pathhelper(repo, f, aopts).linelogpath
                    )
                )
                for f in relpats
            ):
                perfhack = False

    # perfhack: emit paths directory without checking with manifest
    # this can be incorrect if the rev dos not have file.
    if perfhack:
        for p in relpats:
            yield os.path.join(reldir, p)
    else:

        def bad(x, y):
            raise error.Abort("%s: %s" % (x, y))

        ctx = scmutil.revsingle(repo, rev)
        m = scmutil.match(ctx, pats, opts, badfn=bad)
        for p in ctx.walk(m):
            yield p


fastannotatecommandargs = {
    "options": [
        ("r", "rev", ".", _("annotate the specified revision"), _("REV")),
        ("u", "user", None, _("list the author (long with -v)")),
        ("f", "file", None, _("list the filename")),
        ("d", "date", None, _("list the date (short with -q)")),
        ("n", "number", None, _("list the revision number (default)")),
        ("c", "changeset", None, _("list the changeset")),
        ("l", "line-number", None, _("show line number at the first " "appearance")),
        ("e", "deleted", None, _("show deleted lines (slow) (EXPERIMENTAL)")),
        ("", "no-content", None, _("do not show file content (EXPERIMENTAL)")),
        ("", "no-follow", None, _("don't follow copies and renames")),
        (
            "",
            "linear",
            None,
            _(
                "enforce linear history, ignore second parent "
                "of merges (EXPERIMENTAL)"
            ),
        ),
        ("", "long-hash", None, _("show long changeset hash (EXPERIMENTAL)")),
        ("", "rebuild", None, _("rebuild cache even if it exists " "(EXPERIMENTAL)")),
    ]
    + commands.diffwsopts
    + commands.walkopts
    + commands.formatteropts,
    "synopsis": _("[-r REV] [-f] [-a] [-u] [-d] [-n] [-c] [-l] FILE..."),
    "inferrepo": True,
}


def fastannotate(ui, repo, *pats, **opts):
    """show changeset information by line for each file

    List changes in files, showing the revision id responsible for each line.

    This command is useful for discovering when a change was made and by whom.

    By default this command prints revision numbers. If you include --file,
    --user, or --date, the revision number is suppressed unless you also
    include --number. The default format can also be customized by setting
    fastannotate.defaultformat.

    Returns 0 on success.

    .. container:: verbose

        This command uses an implementation different from the vanilla annotate
        command, which may produce slightly different (while still reasonable)
        outputs for some cases.

        Unlike the vanilla anootate, fastannotate follows rename regardless of
        the existence of --file.

        For the best performance when running on a full repo, use -c, -l,
        avoid -u, -d, -n. Use --linear and --no-content to make it even faster.

        For the best performance when running on a shallow (remotefilelog)
        repo, avoid --linear, --no-follow, or any diff options. As the server
        won't be able to populate annotate cache when non-default options
        affecting results are used.
    """
    if not pats:
        raise error.Abort(_("at least one filename or pattern is required"))

    # performance hack: filtered repo can be slow. unfilter by default.
    if ui.configbool("fastannotate", "unfilteredrepo", True):
        repo = repo.unfiltered()

    rev = opts.get("rev", ".")
    rebuild = opts.get("rebuild", False)

    diffopts = patch.difffeatureopts(ui, opts, section="annotate", whitespace=True)
    aopts = facontext.annotateopts(
        diffopts=diffopts,
        followmerge=not opts.get("linear", False),
        followrename=not opts.get("no_follow", False),
    )

    if not any(opts.get(s) for s in ["user", "date", "file", "number", "changeset"]):
        # default 'number' for compatibility. but fastannotate is more
        # efficient with "changeset", "line-number" and "no-content".
        for name in ui.configlist("fastannotate", "defaultformat", ["number"]):
            opts[name] = True

    ui.pager("fastannotate")
    template = opts.get("template")
    if template == "json":
        formatter = faformatter.jsonformatter(ui, repo, opts)
    else:
        formatter = faformatter.defaultformatter(ui, repo, opts)
    showdeleted = opts.get("deleted", False)
    showlines = not bool(opts.get("no_content"))
    showpath = opts.get("file", False)

    # find the head of the main (master) branch
    master = ui.config("fastannotate", "mainbranch") or rev

    # paths will be used for prefetching and the real annotating
    paths = list(_matchpaths(repo, rev, pats, opts, aopts))

    # for client, prefetch from the server
    if util.safehasattr(repo, "prefetchfastannotate"):
        repo.prefetchfastannotate(paths)

    for path in paths:
        result = lines = existinglines = None
        while True:
            try:
                with facontext.annotatecontext(repo, path, aopts, rebuild) as a:
                    result = a.annotate(
                        rev,
                        master=master,
                        showpath=showpath,
                        showlines=(showlines and not showdeleted),
                    )
                    if showdeleted:
                        existinglines = set((l[0], l[1]) for l in result)
                        result = a.annotatealllines(
                            rev, showpath=showpath, showlines=showlines
                        )
                break
            except (faerror.CannotReuseError, faerror.CorruptedFileError):
                # happens if master moves backwards, or the file was deleted
                # and readded, or renamed to an existing name, or corrupted.
                if rebuild:  # give up since we have tried rebuild already
                    raise
                else:  # try a second time rebuilding the cache (slow)
                    rebuild = True
                    continue

        if showlines:
            result, lines = result

        formatter.write(result, lines, existinglines=existinglines)
    formatter.end()


_newopts = set([])
_knownopts = set(
    [
        opt[1].replace("-", "_")
        for opt in (fastannotatecommandargs["options"] + commands.globalopts)
    ]
)


def _annotatewrapper(orig, ui, repo, *pats, **opts):
    """used by wrapdefault"""
    # we need this hack until the obsstore has 0.0 seconds perf impact
    if ui.configbool("fastannotate", "unfilteredrepo", True):
        repo = repo.unfiltered()

    # treat the file as text (skip the isbinary check)
    if ui.configbool("fastannotate", "forcetext", True):
        opts["text"] = True

    # check if we need to do prefetch (client-side)
    rev = opts.get("rev")
    if util.safehasattr(repo, "prefetchfastannotate") and rev is not None:
        paths = list(_matchpaths(repo, rev, pats, opts))
        repo.prefetchfastannotate(paths)

    return orig(ui, repo, *pats, **opts)


def registercommand():
    """register the fastannotate command"""
    name = "fastannotate|fastblame|fa"
    command(name, **fastannotatecommandargs)(fastannotate)


def wrapdefault():
    """wrap the default annotate command, to be aware of the protocol"""
    extensions.wrapcommand(commands.table, "annotate", _annotatewrapper)


@command(
    "debugbuildannotatecache",
    [("r", "rev", "", _("build up to the specific revision"), _("REV"))]
    + commands.walkopts,
    _("[-r REV] FILE..."),
)
def debugbuildannotatecache(ui, repo, *pats, **opts):
    """incrementally build fastannotate cache up to REV for specified files

    If REV is not specified, use the config 'fastannotate.mainbranch'.

    If fastannotate.client is True, download the annotate cache from the
    server. Otherwise, build the annotate cache locally.

    The annotate cache will be built using the default diff and follow
    options and lives in '.hg/fastannotate/default'.
    """
    rev = opts.get("REV") or ui.config("fastannotate", "mainbranch")
    if not rev:
        raise error.Abort(
            _("you need to provide a revision"),
            hint=_("set fastannotate.mainbranch or use --rev"),
        )
    if ui.configbool("fastannotate", "unfilteredrepo", True):
        repo = repo.unfiltered()
    ctx = scmutil.revsingle(repo, rev)
    m = scmutil.match(ctx, pats, opts)
    paths = list(ctx.walk(m))
    if util.safehasattr(repo, "prefetchfastannotate"):
        # client
        if opts.get("REV"):
            raise error.Abort(_("--rev cannot be used for client"))
        repo.prefetchfastannotate(paths)
    else:
        # server, or full repo
        with progress.bar(ui, _("building"), total=len(paths)) as prog:
            for i, path in enumerate(paths):
                prog.value = i
                with facontext.annotatecontext(repo, path) as actx:
                    try:
                        if actx.isuptodate(rev):
                            continue
                        actx.annotate(rev, rev)
                    except (faerror.CannotReuseError, faerror.CorruptedFileError):
                        # the cache is broken (could happen with renaming so the
                        # file history gets invalidated). rebuild and try again.
                        ui.debug("fastannotate: %s: rebuilding broken cache\n" % path)
                        actx.rebuild()
                        try:
                            actx.annotate(rev, rev)
                        except Exception as ex:
                            # possibly a bug, but should not stop us from
                            # building cache for other files.
                            ui.warn(
                                _("fastannotate: %s: failed to " "build cache: %r\n")
                                % (path, ex)
                            )

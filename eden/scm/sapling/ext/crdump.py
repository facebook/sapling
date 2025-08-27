# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# crdump.py - dump changesets information to filesystem
#


import json
import re
import shutil
import tempfile
from os import path

from sapling import error, extensions, registrar, scmutil
from sapling.i18n import _
from sapling.node import hex

from . import commitcloud

DIFFERENTIAL_REGEX = re.compile(
    "Differential Revision: http.+?/"  # Line start, URL
    "D(?P<id>[0-9]+)"  # Differential ID, just numeric part
)
cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "debugcrdump",
    [
        ("r", "rev", [], _("revisions to dump")),
        # We use 1<<15 for "as much context as possible"
        ("U", "unified", 1 << 15, _("number of lines of context to show"), _("NUM")),
        ("l", "lfs", False, "Provide sha256 for lfs files instead of dumping"),
        ("", "obsolete", False, "add obsolete markers related to the given revisions"),
        ("", "nobinary", False, "do not dump binary files"),
    ],
    _("@prog@ debugcrdump [OPTION]... [-r] [REV]"),
)
def crdump(ui, repo, *revs, **opts):
    """
    Dump the info about the revisions in format that's friendly for sending the
    patches for code review.

    The output is a JSON list with dictionary for each specified revision: ::

        {
          "output_directory": an output directory for all temporary files
          "commits": [
          {
            "node": commit hash,
            "date": date in format [unixtime, timezone offset],
            "desc": commit message,
            "patch_file": path to file containing patch in unified diff format
                          relative to output_directory,
            "commit_cloud": true if the commit is in commit cloud,
            "files": list of files touched by commit,
            "binary_files": [
              {
                "filename": path to file relative to repo root,
                "old_file": path to file (relative to output_directory) with
                            a dump of the old version of the file,
                "new_file": path to file (relative to output_directory) with
                            a dump of the newversion of the file,
              },
              ...
            ],
            "user": commit author,
            "p1": {
              "node": hash,
              "differential_revision": xxxx
            },
            "public_base": {
              "node": public base commit hash,
              "svnrev": svn revision of public base (if hgsvn repo),
            },
            "obsolete": {
                "date": [
                    time,
                    timezone
                ],
                "flag": marker's flags,
                "metadata": {
                    "operation": changes made,
                    "user": user name
                },
                "prednode": predecessor commit in hash,
                "succnodes": [
                    successors in hash
                ]
            }
          },
          ...
          ]
        }
    """

    revs = list(revs)
    revs.extend(opts["rev"])

    if not revs:
        raise error.Abort(_("revisions must be specified"))
    revs = scmutil.revrange(repo, revs)

    if "unified" in opts:
        contextlines = opts["unified"]

    cdata = []
    outdir = tempfile.mkdtemp(suffix="hg.crdump")
    try:
        # notbackedup is a revset
        notbackedup = revs
        if ui.configbool(
            "crdump", "commitcloud", False
        ) and commitcloud.util.is_supported(repo):
            try:
                oldquiet = repo.ui.quiet
                # Silence any output from commitcloud
                repo.ui.quiet = True
                _backedup, notbackedup = commitcloud.upload.upload(repo, revs)
                # Convert nodes back to revs for the below check.
                notbackedup = repo.revs("%ln", notbackedup)
            except Exception as ex:
                if ui.configbool("crdump", "commitcloudrequired"):
                    raise
                # Don't let commit cloud exceptions block crdump, just log
                # the exception.
                ui.log_exception(
                    exception_type=type(ex).__name__,
                    exception_msg=str(ex),
                    fatal="false",
                    source="crdump_commitcloud",
                )
            finally:
                repo.ui.quiet = oldquiet

            if notbackedup and ui.configbool("crdump", "commitcloudrequired"):
                raise error.Abort(
                    "failed to upload commits to commit cloud: %s"
                    % ", ".join(str(repo[rev]) for rev in notbackedup)
                )

        for rev in revs:
            ctx = repo[rev]
            rdata = {
                "node": hex(ctx.node()),
                "date": list(map(int, ctx.date())),
                "desc": ctx.description(),
                "files": ctx.files(),
                "p1": {"node": ctx.p1().hex()},
                "user": ctx.user(),
                "bookmarks": ctx.bookmarks(),
                "commit_cloud": False if ctx.rev() in notbackedup else True,
                "manifest_node": hex(ctx.manifestnode()),
            }
            if not ctx.p1().ispublic():
                # we need this only if parent is in the same draft stack
                rdata["p1"]["differential_revision"] = phabricatorrevision(ctx.p1())

            rdata["branch"] = ""

            pbctx = scmutil.publicbase(repo, ctx)
            if pbctx:
                rdata["public_base"] = {"node": hex(pbctx.node())}
                try:
                    globalrevs = extensions.find("globalrevs")
                    globalrev = globalrevs.getglobalrev(ui, pbctx)
                    rdata["public_base"]["svnrev"] = globalrev
                except KeyError:
                    pass

                if extensions.isenabled(ui, "remotenames"):
                    downstreams = repo.revs(
                        "sort(%n:: & remotebookmark())", pbctx.node()
                    )
                    downstreambookmarks = []
                    for r in downstreams:
                        downstreambookmarks.extend(
                            repo.names["hoistednames"].names(repo, repo[r].node())
                        )

                    # Caveat: In Sapling it's impossible to know for certain which
                    # remote bookmark a local commit was made against. The best we
                    # can do is a heuristic.  The heuristicis as follows:
                    #   1. If 'master' is in downstreambookmarks, then use it.
                    #   2. Otherwise report the first bookmark as the current branch.
                    #      For draft commit, this should be (best guess) the remote
                    #      bookmark on which the draft commit was based if user didn't
                    #      run `pull` from remote server.
                    if downstreambookmarks:
                        if "master" in downstreambookmarks:
                            rdata["branch"] = "master"
                        else:
                            rdata["branch"] = downstreambookmarks[0]

            rdata["patch_file"] = dumppatch(ui, repo, ctx, outdir, contextlines)
            if not opts["nobinary"]:
                rdata["binary_files"] = dumpbinaryfiles(ui, repo, ctx, outdir)
            cdata.append(rdata)

        ui.write(
            json.dumps(
                {"output_directory": outdir, "commits": cdata},
                sort_keys=True,
                indent=4,
                separators=(",", ": "),
            )
        )
        ui.write("\n")
    except Exception:
        shutil.rmtree(outdir)
        raise


def dumppatch(ui, repo, ctx, outdir, contextlines):
    chunks = ctx.diff(git=True, unified=contextlines, binary=False)
    patchfile = "%s.patch" % hex(ctx.node())
    with open(path.join(outdir, patchfile), "wb") as f:
        for chunk in chunks:
            f.write(chunk)
    return patchfile


def dumpfctx(outdir, fctx):
    outfile = "%s" % hex(fctx.filenode())
    writepath = path.join(outdir, outfile)
    if not path.isfile(writepath):
        with open(writepath, "wb") as f:
            f.write(fctx.data())
    return outfile


def dumpbinaryfiles(ui, repo, ctx, outdir):
    binaryfiles = []
    pctx = ctx.p1()
    for fname in ctx.files():
        oldfile = newfile = None
        dump = False

        fctx = ctx[fname] if fname in ctx else None
        if fctx and fctx.isbinary():
            dump = True

        pfctx = pctx[fname] if fname in pctx else None
        if pfctx and pfctx.isbinary():
            dump = True

        if dump:
            if fctx:
                newfile = dumpfctx(outdir, fctx)
            if pfctx:
                oldfile = dumpfctx(outdir, pfctx)

        if dump:
            binaryfiles.append(
                {"file_name": fname, "old_file": oldfile, "new_file": newfile}
            )

    return binaryfiles


def phabricatorrevision(ctx):
    match = DIFFERENTIAL_REGEX.search(ctx.description())
    return match.group(1) if match else ""


def dumpmarkers(rawmarkers):
    markers = []
    for rm in rawmarkers:
        marker = {
            "date": rm.date(),
            "flag": rm.flags(),
            "metadata": rm.metadata(),
            "prednode": hex(rm.prednode()),
        }
        if rm.succnodes():
            marker["succnodes"] = list(map(hex, rm.succnodes()))
        if rm.parentnodes():
            marker["parents"] = list(map(hex, rm.parentnodes()))

        markers.append(marker)

    return markers

# crdump.py - dump changesets information to filesystem
#
from __future__ import absolute_import

import json, re, shutil, tempfile
from os import path

from mercurial import (
    error,
    extensions,
    phases,
    registrar,
    scmutil,
)

from mercurial.i18n import _
from mercurial.node import hex

DIFFERENTIAL_REGEX = re.compile(
    'Differential Revision: http.+?/'  # Line start, URL
    'D(?P<id>[0-9]+)',  # Differential ID, just numeric part
    flags = re.LOCALE
)
cmdtable = {}
command = registrar.command(cmdtable)

@command('debugcrdump', [
         ('r', 'rev', [], _("revisions to dump")),
         # We use 1<<15 for "as much context as possible"
         ('U', 'unified',
          1 << 15, _('number of lines of context to show'), _('NUM')),
         ],
         _('hg debugcrdump [OPTION]... [-r] [REV]'))
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
            }
          },
          ...
          ]
        }
    """

    revs = list(revs)
    revs.extend(opts['rev'])

    if not revs:
        raise error.Abort(_('revisions must be specified'))
    revs = scmutil.revrange(repo, revs)

    if 'unified' in opts:
        contextlines = opts['unified']

    cdata = []
    outdir = tempfile.mkdtemp(suffix='hg.crdump')
    try:
        for rev in revs:
            ctx = repo[rev]
            rdata = {
                'node': hex(ctx.node()),
                'date': map(int, ctx.date()),
                'desc': ctx.description(),
                'files': ctx.files(),
                'p1': {
                    'node': ctx.parents()[0].hex(),
                },
                'user': ctx.user(),
            }
            if ctx.parents()[0].phase() != phases.public:
                # we need this only if parent is in the same draft stack
                rdata['p1']['differential_revision'] = \
                    phabricatorrevision(ctx.parents()[0])

            pbctx = publicbase(repo, ctx)
            if pbctx:
                rdata['public_base'] = {
                    'node': hex(pbctx.node()),
                }
                try:
                    hgsubversion = extensions.find('hgsubversion')
                    svnrev = hgsubversion.util.getsvnrev(pbctx)
                    # There are no abstractions in hgsubversion for doing
                    # it see hgsubversion/svncommands.py:267
                    rdata['public_base']['svnrev'] = \
                        svnrev.split('@')[1] if svnrev else None
                except KeyError:
                    pass
            rdata['patch_file'] = dumppatch(ui, repo, ctx, outdir, contextlines)
            rdata['binary_files'] = dumpbinaryfiles(ui, repo, ctx, outdir)
            cdata.append(rdata)

        ui.write(json.dumps({
            'output_directory': outdir,
            'commits': cdata,
        }, sort_keys=True, indent=4, separators=(',', ': ')))
        ui.write('\n')
    except Exception as e:
        shutil.rmtree(outdir)
        raise e

def dumppatch(ui, repo, ctx, outdir, contextlines):
    chunks = ctx.diff(git=True, unified=contextlines, binary=False)
    patchfile = '%s.patch' % hex(ctx.node())
    with open(path.join(outdir, patchfile), 'wb') as f:
        for chunk in chunks:
            f.write(chunk)
    return patchfile

def dumpfctx(outdir, fctx):
    outfile = '%s' % hex(fctx.filenode())
    writepath = path.join(outdir, outfile)
    if not path.isfile(writepath):
        with open(writepath, 'wb') as f:
            f.write(fctx.data())
    return outfile

def dumpbinaryfiles(ui, repo, ctx, outdir):
    binaryfiles = []
    pctx = ctx.parents()[0]
    for fname in ctx.files():
        oldfile = newfile = None
        dump = False

        fctx = ctx[fname] if fname in ctx else None
        pfctx = pctx[fname] if fname in pctx else None

        # if one of the versions is binary file the whole change will show
        # up as binary in diff output so we need to dump both versions
        if fctx and fctx.isbinary():
            dump = True
        if pfctx and pfctx.isbinary():
            dump = True

        if dump:
            if fctx:
                newfile = dumpfctx(outdir, fctx)
            if pfctx:
                oldfile = dumpfctx(outdir, pfctx)
            binaryfiles.append({
                'file_name': fname,
                'old_file': oldfile,
                'new_file': newfile,
            })

    return binaryfiles

def phabricatorrevision(ctx):
    match = DIFFERENTIAL_REGEX.search(ctx.description())
    return match.group(1) if match else ''

def publicbase(repo, ctx):
    base = repo.revs('last(::%d & public())', ctx.rev())
    if len(base):
        return repo[base.first()]
    return None

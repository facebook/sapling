# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import re

from mercurial import (
    context,
)

from . import importer, lfs, p4

SYNC_COMMIT_MSG = 'p4fastimport synchronizing client view'
P4_ADMIN_USER = 'p4admin'

def get_filelogs_to_sync(ui, client, repo, p1ctx, cl, p4filelogs):
    p1 = repo[p1ctx.node()]
    hgfilelogs = p1.manifest().copy()
    p4flmapping = {p4fl.depotfile: p4fl for p4fl in p4filelogs}
    headcl_to_origcl = {}
    # {hgpath: (p4filelog, p4cl)}
    files_add = {}
    # a list of hg paths
    files_del = []
    ui.debug('%d p4 filelogs to read\n' % (len(p4filelogs)))

    mapping = p4.parse_where_multiple(client, p4flmapping.keys())
    for p4file, hgfile in mapping.items():
        if hgfile not in hgfilelogs:
            p4fl = p4flmapping[p4file]
            headcl = p4fl.getheadchangelist(cl)
            origcl = headcl_to_origcl.get(headcl)
            if origcl is None:
                origcl = p4.getorigcl(client, headcl)
                headcl_to_origcl[headcl] = origcl
            p4cl = p4.P4Changelist(int(origcl), int(headcl), None, None)
            files_add[hgfile] = (p4fl, p4cl)

    files_del = list(set(hgfilelogs) - set(mapping.values()))

    return files_add, files_del

class SyncImporter(object):
    def __init__(self, ui, repo, ctx, storepath, cl, filesadd, filesdel):
        self._ui = ui
        self._repo = repo
        self._node = self._repo[ctx].node()
        self._storepath = storepath
        self._cl = cl
        self._filesadd = filesadd
        self._filesdel = filesdel

    def sync_commit(self):
        node = self._create_commit()
        largefiles = self._get_large_files(node)
        return node, largefiles

    def _get_large_files(self, node):
        largefiles = []
        ctx = self._repo[node]
        for hgpath, (p4fl, p4cl) in self._filesadd.items():
            flog = self._repo.file(hgpath)
            fnode = ctx.filenode(hgpath)
            islfs, oid = lfs.getlfsinfo(flog, fnode)
            if islfs:
                largefiles.append((self._cl, p4fl.depotfile, oid))
                self._ui.debug('largefile: %s, oid: %s\n' % (hgpath, oid))
        return largefiles

    def _create_commit(self):

        def getfile(repo, memctx, path):
            if path in self._filesdel:
                # A path that shows up in files (below) but returns None in this
                # function implies a deletion.
                return None

            p4flog, p4cl = self._filesadd[path]
            data, src = importer.get_p4_file_content(
                self._storepath,
                p4flog,
                p4cl,
                skipp4revcheck=True,
            )
            self._ui.debug('file: %s, src: %s\n' % (p4flog._depotfile, src))

            islink = p4flog.issymlink(self._cl)
            if islink:
                # p4 will give us content with a trailing newline, symlinks
                # cannot end with newline
                data = data.rstrip()
            if p4flog.iskeyworded(self._cl):
                data = re.sub(importer.KEYWORD_REGEX, r'$\1$', data)
            isexec=p4flog.isexec(self._cl)

            return context.memfilectx(
                repo,
                memctx,
                path,
                data,
                islink=islink,
                isexec=isexec,
                copied=None,
            )

        files_affected = self._filesadd.keys() + self._filesdel

        return context.memctx(
            self._repo,
            (self._node, None), # parents
            SYNC_COMMIT_MSG,
            files_affected,
            getfile,            # fn - see above
            user=P4_ADMIN_USER,
            date=None,
            extra={'p4fullimportbasechangelist': self._cl},
        ).commit()

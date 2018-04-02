# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import errno
import os

from mercurial.i18n import _
from mercurial import (
    context,
)

from . import importer, lfs, p4

class ChangelistImporter(object):
    def __init__(self, ui, repo, ctx, client, storepath, bookmark):
        self.ui = ui
        self.repo = repo
        self.node = self.repo[ctx].node()
        self.client = client
        self.storepath = storepath
        self.bookmark = bookmark

    def importcl(self, p4cl, bookmark=None):
        try:
            ctx, largefiles = self._import(p4cl)
            self.node = self.repo[ctx].node()
            self._update_bookmark()
            return ctx, largefiles
        except Exception as e:
            self.ui.write_err(_('Failed importing CL%d: %s\n') % (p4cl.cl, e))
            raise

    def _update_bookmark(self):
        if not self.bookmark:
            return
        tr = self.repo.currenttransaction()
        changes = [(self.bookmark, self.node)]
        self.repo._bookmarks.applychanges(self.repo, tr, changes)

    def _import(self, p4cl):
        '''Converts the provided p4 CL into a commit in hg.
        Returns a tuple containing hg node and largefiles for new commit'''
        self.ui.debug('importing CL%d\n' % p4cl.cl)
        fstat = p4.parse_fstat(p4cl.cl, self.client)
        added_or_modified = []
        removed = set()
        p4flogs = {}
        for info in fstat:
            action = info['action']
            p4path = info['depotFile']
            data = {p4cl.cl: {'action': action, 'type': info['type']}}
            hgpath = importer.relpath(self.client, p4path)
            p4flogs[hgpath] = p4.P4Filelog(p4path, data)

            if action in p4.ACTION_DELETE + p4.ACTION_ARCHIVE:
                removed.add(hgpath)
            else:
                added_or_modified.append((p4path, hgpath))

        moved = self._get_move_info(p4cl)
        node = self._create_commit(p4cl, p4flogs, removed, moved)
        largefiles = self._get_largefiles(p4cl, added_or_modified)

        return node, largefiles

    def _get_largefiles(self, p4cl, files):
        largefiles = []
        for p4path, hgpath in files:
            flog = self.repo.file(hgpath)
            node = flog.tip()
            islfs, oid = lfs.getlfsinfo(flog, node)
            if islfs:
                largefiles.append((p4cl.cl, p4path, oid))
                self.ui.debug('largefile: %s, oid: %s\n' % (hgpath, oid))
        return largefiles

    def _safe_open(self, path):
        '''Returns file handle for path, creating non-existing directories'''
        dirname = os.path.dirname(path)
        try:
            os.makedirs(dirname)
        except OSError as err:
            if err.errno != errno.EEXIST or not os.path.isdir(dirname):
                raise err
        return open(path, 'w')

    def _get_move_info(self, p4cl):
        '''Returns a dict where entries are (dst, src)'''
        moves = {}
        for filename, info in p4cl.parsed['files'].items():
            src = info.get('src')
            if src:
                hgsrc = importer.relpath(self.client, src)
                hgdst = importer.relpath(self.client, filename)
                moves[hgdst] = hgsrc
        return moves

    def _create_commit(self, p4cl, p4flogs, removed, moved):
        '''Uses a memory context to commit files into the repo'''
        def getfile(repo, memctx, path):
            if path in removed:
                # A path that shows up in files (below) but returns None in this
                # function implies a deletion.
                return None

            p4flog = p4flogs[path]
            data = p4.get_file(p4flog._depotfile, clnum=p4cl.cl)
            islink = p4flog.issymlink(p4cl.cl)
            if islink:
                # p4 will give us content with a trailing newline, symlinks
                # cannot end with newline
                data = data.rstrip()

            return context.memfilectx(
                repo,
                memctx,
                path,
                data,
                islink=islink,
                isexec=p4flog.isexec(p4cl.cl),
                copied=moved.get(path),
            )

        return context.memctx(
            self.repo,                        # repository
            (self.node, None),                # parents
            p4cl.description,                 # commit message
            p4flogs.keys(),                   # files affected by this change
            getfile,                          # fn - see above
            user=p4cl.user,                   # commit author
            date=p4cl.hgdate,                 # commit date
            extra={'p4changelist': p4cl.cl},  # commit extras
        ).commit()

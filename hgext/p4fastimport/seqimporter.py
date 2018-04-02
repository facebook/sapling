# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import errno
import os

from mercurial.i18n import _
from mercurial import (
    bookmarks,
    commands,
)

from . import importer, lfs, p4

MoveInfo = collections.namedtuple('MoveInfo', ['src', 'dst'])

class ChangelistImporter(object):
    def __init__(self, ui, repo, client, storepath, bookmark):
        self.ui = ui
        self.repo = repo
        self.client = client
        self.storepath = storepath
        self.bookmark = bookmark

    def importcl(self, p4cl, bookmark=None):
        try:
            node, largefiles = self._import(p4cl)
            self._update_bookmark(node)
            return node, largefiles
        except Exception as e:
            self.ui.write_err(_('Failed importing CL%d: %s\n') % (p4cl.cl, e))
            raise

    def _update_bookmark(self, rev):
        if not self.bookmark:
            return
        tr = self.repo.currenttransaction()
        bookmarks.addbookmarks(self.repo, tr, [self.bookmark], rev, force=True)

    def _import(self, p4cl):
        '''Converts the provided p4 CL into a commit in hg.
        Returns a tuple containing hg node and largefiles for new commit'''
        self.ui.debug('importing CL%d\n' % p4cl.cl)
        fstat = p4.parse_fstat(p4cl.cl, self.client)
        added, removed = [], []
        added_or_modified = []
        for info in fstat:
            action = info['action']
            p4path = info['depotFile']
            data = {p4cl.cl: {'action': action, 'type': info['type']}}
            p4flog = p4.P4Filelog(p4path, data)
            hgpath = importer.relpath(self.client, p4path)
            if action in p4.ACTION_DELETE + p4.ACTION_ARCHIVE:
                removed.append(hgpath)
            else:
                added_or_modified.append((p4path, hgpath))
                file_content = self._get_file_content(p4path, p4cl.cl)
                if p4flog.issymlink(p4cl.cl):
                    target = file_content.rstrip()
                    os.symlink(target, hgpath)
                else:
                    if os.path.islink(hgpath):
                        os.remove(hgpath)
                    with self._safe_open(hgpath) as f:
                        f.write(file_content)
                if action in p4.ACTION_ADD:
                    added.append(hgpath)

        moved = self._get_move_info(p4cl)
        move_dsts = set(mi.dst for mi in moved)
        added = [fname for fname in added if fname not in move_dsts]

        node = self._create_commit(p4cl, added, moved, removed)
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
        '''Returns a list of MoveInfo, i.e. (src, dst) for each moved file'''
        moves = []
        for filename, info in p4cl.parsed['files'].items():
            src = info.get('src')
            if src:
                hgsrc = importer.relpath(self.client, src)
                hgdst = importer.relpath(self.client, filename)
                moves.append(MoveInfo(hgsrc, hgdst))
        return moves

    def _get_file_content(self, p4path, clnum):
        '''Returns file content for file in p4path'''
        # TODO try to get file from local stores instead of resorting to
        # p4 print, similar to what importer.FileImporter does
        return p4.get_file(p4path, clnum=clnum)

    def _create_commit(self, p4cl, added, moved, removed):
        '''Performs all hg add/mv/rm and creates a commit'''
        if added:
            commands.add(self.ui, self.repo, *added)
        for mi in moved:
            commands.copy(self.ui, self.repo, mi.src, mi.dst, after=True)
        if removed:
            commands.remove(self.ui, self.repo, *removed)

        return self.repo.commit(
            text=p4cl.description,
            date=p4cl.hgdate,
            user=p4cl.user,
            extra={'p4changelist': p4cl.cl},
        )

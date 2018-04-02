# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import errno
import os

from mercurial.i18n import _
from mercurial import (
    commands,
    lock as lockmod,
)

from . import importer, p4

MoveInfo = collections.namedtuple('MoveInfo', ['src', 'dst'])

class ChangelistImporter(object):
    def __init__(self, ui, repo, client, storepath):
        self.ui = ui
        self.repo = repo
        self.client = client
        self.storepath = storepath

    def importcl(self, p4cl):
        wlock = lock = None
        try:
            wlock = self.repo.wlock()
            lock = self.repo.lock()
            return self._import(p4cl)
        except Exception as e:
            self.ui.write_err(_('Failed importing CL%d: %s\n') % (p4cl.cl, e))
            raise
        finally:
            lockmod.release(lock, wlock)

    def _import(self, p4cl):
        '''Converts the provided p4 CL into a commit in hg.
        Returns a tuple containing the hg node and from the corresponding
        commit and the list of largefiles that were in this commit'''
        self.ui.debug('importing CL%d\n' % p4cl.cl)
        fstat = p4.parse_fstat(p4cl.cl, self.client)
        added, removed = [], []
        for info in fstat:
            action = info['action']
            p4path = info['depotFile']
            hgpath = importer.relpath(self.client, p4path)
            if action in p4.ACTION_DELETE + p4.ACTION_ARCHIVE:
                removed.append(hgpath)
            else:
                with self._safe_open(hgpath) as f:
                    f.write(self._get_file_content(p4path, p4cl.cl))
                if action in p4.ACTION_ADD:
                    added.append(hgpath)

        moved = self._get_move_info(p4cl)
        move_dsts = set(mi.dst for mi in moved)
        added = [fname for fname in added if fname not in move_dsts]

        node = self._create_commit(p4cl, added, moved, removed)
        # TODO properly handle large files (second return here)
        largefiles = []
        return node, largefiles

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

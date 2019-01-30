# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import re

from edenscm.mercurial import context

from . import importer, lfs, p4


SYNC_COMMIT_MSG = "p4fastimport synchronizing client view"
P4_ADMIN_USER = "p4admin"


def get_filelogs_to_sync(ui, oldclient, oldcl, newclient, newcl):
    newp4filelogs = p4.get_filelogs_at_cl(newclient, newcl)
    newp4flmapping = {p4fl.depotfile: p4fl for p4fl in newp4filelogs}
    newp4filepaths = set(newp4flmapping.keys())

    oldp4filelogs = p4.get_filelogs_at_cl(oldclient, oldcl)
    oldp4filepaths = set(p4fl.depotfile for p4fl in oldp4filelogs)

    addp4filepaths = newp4filepaths - oldp4filepaths
    delp4filepaths = oldp4filepaths - newp4filepaths

    num_addfiles = len(addp4filepaths)
    num_delfiles = len(delp4filepaths)
    ui.debug("%d added files\n%d removed files\n" % (num_addfiles, num_delfiles))

    headcl_to_origcl = {}
    # {hgpath: (p4filelog, p4cl)}
    files_add = {}
    addfilesmapping = p4.parse_where_multiple(newclient, addp4filepaths)
    newp4flmapping = {path.lower(): filelog for path, filelog in newp4flmapping.items()}
    for p4path, hgpath in addfilesmapping.items():
        p4fl = newp4flmapping[p4path.lower()]
        headcl = p4fl.getheadchangelist(newcl)
        origcl = headcl_to_origcl.get(headcl)
        if origcl is None:
            origcl = p4.getorigcl(newclient, headcl)
            headcl_to_origcl[headcl] = origcl
        p4cl = p4.P4Changelist(int(origcl), int(headcl), None, None)
        files_add[hgpath] = (p4fl, p4cl)
    delfilesmapping = p4.parse_where_multiple(oldclient, delp4filepaths)
    files_del = delfilesmapping.values()
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
                self._ui.debug("largefile: %s, oid: %s\n" % (hgpath, oid))
        return largefiles

    def _create_commit(self):
        def getfile(repo, memctx, path):
            if path in self._filesdel:
                # A path that shows up in files (below) but returns None in this
                # function implies a deletion.
                return None

            p4flog, p4cl = self._filesadd[path]
            data, src = importer.get_p4_file_content(
                self._storepath, p4flog, p4cl, skipp4revcheck=True
            )
            self._ui.debug("file: %s, src: %s\n" % (p4flog._depotfile, src))

            islink = p4flog.issymlink(self._cl)
            if islink:
                # p4 will give us content with a trailing newline, symlinks
                # cannot end with newline
                data = data.rstrip()
            if p4flog.iskeyworded(self._cl):
                data = re.sub(importer.KEYWORD_REGEX, r"$\1$", data)
            isexec = p4flog.isexec(self._cl)

            return context.memfilectx(
                repo, memctx, path, data, islink=islink, isexec=isexec, copied=None
            )

        files_affected = self._filesadd.keys() + self._filesdel

        return context.memctx(
            self._repo,
            (self._node, None),  # parents
            SYNC_COMMIT_MSG,
            files_affected,
            getfile,  # fn - see above
            user=P4_ADMIN_USER,
            date=None,
            extra={"p4fullimportbasechangelist": self._cl},
        ).commit()

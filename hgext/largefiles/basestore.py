# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''base class for store implementations and store-related utility code'''
from __future__ import absolute_import

from mercurial.i18n import _

from mercurial import node, util

from . import lfutil

class StoreError(Exception):
    '''Raised when there is a problem getting files from or putting
    files to a central store.'''
    def __init__(self, filename, hash, url, detail):
        self.filename = filename
        self.hash = hash
        self.url = url
        self.detail = detail

    def longmessage(self):
        return (_("error getting id %s from url %s for file %s: %s\n") %
                 (self.hash, util.hidepassword(self.url), self.filename,
                  self.detail))

    def __str__(self):
        return "%s: %s" % (util.hidepassword(self.url), self.detail)

class basestore(object):
    def __init__(self, ui, repo, url):
        self.ui = ui
        self.repo = repo
        self.url = url

    def put(self, source, hash):
        '''Put source file into the store so it can be retrieved by hash.'''
        raise NotImplementedError('abstract method')

    def exists(self, hashes):
        '''Check to see if the store contains the given hashes. Given an
        iterable of hashes it returns a mapping from hash to bool.'''
        raise NotImplementedError('abstract method')

    def get(self, files):
        '''Get the specified largefiles from the store and write to local
        files under repo.root.  files is a list of (filename, hash)
        tuples.  Return (success, missing), lists of files successfully
        downloaded and those not found in the store.  success is a list
        of (filename, hash) tuples; missing is a list of filenames that
        we could not get.  (The detailed error message will already have
        been presented to the user, so missing is just supplied as a
        summary.)'''
        success = []
        missing = []
        ui = self.ui

        at = 0
        available = self.exists(set(hash for (_filename, hash) in files))
        for filename, hash in files:
            ui.progress(_('getting largefiles'), at, unit=_('files'),
                total=len(files))
            at += 1
            ui.note(_('getting %s:%s\n') % (filename, hash))

            if not available.get(hash):
                ui.warn(_('%s: largefile %s not available from %s\n')
                        % (filename, hash, util.hidepassword(self.url)))
                missing.append(filename)
                continue

            if self._gethash(filename, hash):
                success.append((filename, hash))
            else:
                missing.append(filename)

        ui.progress(_('getting largefiles'), None)
        return (success, missing)

    def _gethash(self, filename, hash):
        """Get file with the provided hash and store it in the local repo's
        store and in the usercache.
        filename is for informational messages only.
        """
        util.makedirs(lfutil.storepath(self.repo, ''))
        storefilename = lfutil.storepath(self.repo, hash)

        tmpname = storefilename + '.tmp'
        with util.atomictempfile(tmpname,
                createmode=self.repo.store.createmode) as tmpfile:
            try:
                gothash = self._getfile(tmpfile, filename, hash)
            except StoreError as err:
                self.ui.warn(err.longmessage())
                gothash = ""

        if gothash != hash:
            if gothash != "":
                self.ui.warn(_('%s: data corruption (expected %s, got %s)\n')
                             % (filename, hash, gothash))
            util.unlink(tmpname)
            return False

        util.rename(tmpname, storefilename)
        lfutil.linktousercache(self.repo, hash)
        return True

    def verify(self, revs, contents=False):
        '''Verify the existence (and, optionally, contents) of every big
        file revision referenced by every changeset in revs.
        Return 0 if all is well, non-zero on any errors.'''

        self.ui.status(_('searching %d changesets for largefiles\n') %
                       len(revs))
        verified = set()                # set of (filename, filenode) tuples
        filestocheck = []               # list of (cset, filename, expectedhash)
        for rev in revs:
            cctx = self.repo[rev]
            cset = "%d:%s" % (cctx.rev(), node.short(cctx.node()))

            for standin in cctx:
                filename = lfutil.splitstandin(standin)
                if filename:
                    fctx = cctx[standin]
                    key = (filename, fctx.filenode())
                    if key not in verified:
                        verified.add(key)
                        expectedhash = lfutil.readasstandin(fctx)
                        filestocheck.append((cset, filename, expectedhash))

        failed = self._verifyfiles(contents, filestocheck)

        numrevs = len(verified)
        numlfiles = len(set([fname for (fname, fnode) in verified]))
        if contents:
            self.ui.status(
                _('verified contents of %d revisions of %d largefiles\n')
                % (numrevs, numlfiles))
        else:
            self.ui.status(
                _('verified existence of %d revisions of %d largefiles\n')
                % (numrevs, numlfiles))
        return int(failed)

    def _getfile(self, tmpfile, filename, hash):
        '''Fetch one revision of one file from the store and write it
        to tmpfile.  Compute the hash of the file on-the-fly as it
        downloads and return the hash.  Close tmpfile.  Raise
        StoreError if unable to download the file (e.g. it does not
        exist in the store).'''
        raise NotImplementedError('abstract method')

    def _verifyfiles(self, contents, filestocheck):
        '''Perform the actual verification of files in the store.
        'contents' controls verification of content hash.
        'filestocheck' is list of files to check.
        Returns _true_ if any problems are found!
        '''
        raise NotImplementedError('abstract method')

# crecord.py
#
# Copyright 2008 Mark Edgington <edgimar@gmail.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.
#
# Much of this extension is based on Bryan O'Sullivan's record extension.

'''text-gui based change selection during commit or qrefresh'''
from mercurial.i18n import _
from mercurial import cmdutil, hg, mdiff, patch
from mercurial import util
import cStringIO
import errno
import os
import tempfile

import crpatch
import chunk_selector

def dorecord(ui, repo, commitfunc, *pats, **opts):
    try:
        if not ui.interactive():
            raise util.Abort(_('running non-interactively, use commit instead'))
    except TypeError: # backwards compatibility with hg 1.1
        if not ui.interactive:
            raise util.Abort(_('running non-interactively, use commit instead'))

    def recordfunc(ui, repo, message, match, opts):
        """This is generic record driver.

        Its job is to interactively filter local changes, and accordingly
        prepare working dir into a state, where the job can be delegated to
        non-interactive commit command such as 'commit' or 'qrefresh'.

        After the actual job is done by non-interactive command, working dir
        state is restored to original.

        In the end we'll record interesting changes, and everything else will be
        left in place, so the user can continue his work.
        """

        merge = len(repo[None].parents()) > 1
        if merge:
            raise util.Abort(_('cannot partially commit a merge '
                               '(use hg commit instead)'))

        changes = repo.status(match=match)[:3]
        diffopts = mdiff.diffopts(git=True, nodates=True)
        chunks = patch.diff(repo, changes=changes, opts=diffopts)
        fp = cStringIO.StringIO()
        fp.write(''.join(chunks))
        fp.seek(0)

        # 1. filter patch, so we have intending-to apply subset of it
        chunks = crpatch.filterpatch(opts,
                                     crpatch.parsepatch(changes, fp),
                                     chunk_selector.chunkselector, ui)
        del fp

        contenders = set()
        for h in chunks:
            try:
                contenders.update(set(h.files()))
            except AttributeError:
                pass

        changed = changes[0] + changes[1] + changes[2]
        newfiles = [f for f in changed if f in contenders]

        if not newfiles:
            ui.status(_('no changes to record\n'))
            return 0

        modified = set(changes[0])

        # 2. backup changed files, so we can restore them in the end
        backups = {}
        backupdir = repo.join('record-backups')
        try:
            os.mkdir(backupdir)
        except OSError, err:
            if err.errno != errno.EEXIST:
                raise
        try:
            # backup continues
            for f in newfiles:
                if f not in modified:
                    continue
                fd, tmpname = tempfile.mkstemp(prefix=f.replace('/', '_')+'.',
                                               dir=backupdir)
                os.close(fd)
                ui.debug('backup %r as %r\n' % (f, tmpname))
                util.copyfile(repo.wjoin(f), tmpname)
                backups[f] = tmpname

            fp = cStringIO.StringIO()
            for c in chunks:
                if c.filename() in backups:
                    c.write(fp)
            dopatch = fp.tell()
            fp.seek(0)

            # 2.5 optionally review / modify patch in text editor
            if opts['crecord_reviewpatch']:
                patchtext = fp.read()
                reviewedpatch = ui.edit(patchtext, "")
                fp.truncate(0)
                fp.write(reviewedpatch)
                fp.seek(0)

            # 3a. apply filtered patch to clean repo  (clean)
            if backups:
                hg.revert(repo, repo.dirstate.parents()[0],
                          lambda key: key in backups)

            # 3b. (apply)
            if dopatch:
                try:
                    ui.debug('applying patch\n')
                    ui.debug(fp.getvalue())
                    if hasattr(patch, 'workingbackend'): # detect 1.9
                        patch.internalpatch(ui, repo, fp, strip=1, eolmode=None)
                    else:
                        pfiles = {}
                        try:
                            patch.internalpatch(ui, repo, fp, 1, eolmode=None)
                        except (TypeError, AttributeError): # pre 17cea10c343e
                            try:
                                patch.internalpatch(ui, repo, fp, 1, repo.root,
                                                    eolmode=None)
                            except (TypeError, AttributeError): # pre 00a881581400
                                try:
                                    patch.internalpatch(fp, ui, 1, repo.root,
                                                        files=pfiles, eolmode=None)
                                except TypeError: # backwards compatible with hg 1.1
                                    patch.internalpatch(fp, ui, 1,
                                                        repo.root, files=pfiles)
                        try:
                            cmdutil.updatedir(ui, repo, pfiles)
                        except AttributeError:
                            try:
                                patch.updatedir(ui, repo, pfiles)
                            except AttributeError:
                                # from 00a881581400 onwards
                                pass
                except patch.PatchError, err:
                    s = str(err)
                    if s:
                        raise util.Abort(s)
                    else:
                        raise util.Abort(_('patch failed to apply'))
            del fp

            # 4. We prepared working directory according to filtered patch.
            #    Now is the time to delegate the job to commit/qrefresh or the like!

            # it is important to first chdir to repo root -- we'll call a
            # highlevel command with list of pathnames relative to repo root
            cwd = os.getcwd()
            os.chdir(repo.root)
            try:
                commitfunc(ui, repo, *newfiles, **opts)
            finally:
                os.chdir(cwd)

            return 0
        finally:
            # 5. finally restore backed-up files
            try:
                for realname, tmpname in backups.iteritems():
                    ui.debug('restoring %r to %r\n' % (tmpname, realname))
                    util.copyfile(tmpname, repo.wjoin(realname))
                    os.unlink(tmpname)
                os.rmdir(backupdir)
            except OSError:
                pass
    return cmdutil.commit(ui, repo, recordfunc, pats, opts)

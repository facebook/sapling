# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import shutil
import struct

from .i18n import _
from .node import (
    bin,
    hex,
    nullhex,
    nullid,
    nullrev,
)
from . import (
    copies,
    destutil,
    error,
    filemerge,
    obsolete,
    scmutil,
    subrepo,
    util,
    worker,
)

_pack = struct.pack
_unpack = struct.unpack

def _droponode(data):
    # used for compatibility for v1
    bits = data.split('\0')
    bits = bits[:-2] + bits[-1:]
    return '\0'.join(bits)

class mergestate(object):
    '''track 3-way merge state of individual files

    The merge state is stored on disk when needed. Two files are used: one with
    an old format (version 1), and one with a new format (version 2). Version 2
    stores a superset of the data in version 1, including new kinds of records
    in the future. For more about the new format, see the documentation for
    `_readrecordsv2`.

    Each record can contain arbitrary content, and has an associated type. This
    `type` should be a letter. If `type` is uppercase, the record is mandatory:
    versions of Mercurial that don't support it should abort. If `type` is
    lowercase, the record can be safely ignored.

    Currently known records:

    L: the node of the "local" part of the merge (hexified version)
    O: the node of the "other" part of the merge (hexified version)
    F: a file to be merged entry
    C: a change/delete or delete/change conflict
    D: a file that the external merge driver will merge internally
       (experimental)
    m: the external merge driver defined for this merge plus its run state
       (experimental)
    f: a (filename, dictonary) tuple of optional values for a given file
    X: unsupported mandatory record type (used in tests)
    x: unsupported advisory record type (used in tests)
    l: the labels for the parts of the merge.

    Merge driver run states (experimental):
    u: driver-resolved files unmarked -- needs to be run next time we're about
       to resolve or commit
    m: driver-resolved files marked -- only needs to be run before commit
    s: success/skipped -- does not need to be run any more

    '''
    statepathv1 = 'merge/state'
    statepathv2 = 'merge/state2'

    @staticmethod
    def clean(repo, node=None, other=None, labels=None):
        """Initialize a brand new merge state, removing any existing state on
        disk."""
        ms = mergestate(repo)
        ms.reset(node, other, labels)
        return ms

    @staticmethod
    def read(repo):
        """Initialize the merge state, reading it from disk."""
        ms = mergestate(repo)
        ms._read()
        return ms

    def __init__(self, repo):
        """Initialize the merge state.

        Do not use this directly! Instead call read() or clean()."""
        self._repo = repo
        self._dirty = False
        self._labels = None

    def reset(self, node=None, other=None, labels=None):
        self._state = {}
        self._stateextras = {}
        self._local = None
        self._other = None
        self._labels = labels
        for var in ('localctx', 'otherctx'):
            if var in vars(self):
                delattr(self, var)
        if node:
            self._local = node
            self._other = other
        self._readmergedriver = None
        if self.mergedriver:
            self._mdstate = 's'
        else:
            self._mdstate = 'u'
        shutil.rmtree(self._repo.join('merge'), True)
        self._results = {}
        self._dirty = False

    def _read(self):
        """Analyse each record content to restore a serialized state from disk

        This function process "record" entry produced by the de-serialization
        of on disk file.
        """
        self._state = {}
        self._stateextras = {}
        self._local = None
        self._other = None
        for var in ('localctx', 'otherctx'):
            if var in vars(self):
                delattr(self, var)
        self._readmergedriver = None
        self._mdstate = 's'
        unsupported = set()
        records = self._readrecords()
        for rtype, record in records:
            if rtype == 'L':
                self._local = bin(record)
            elif rtype == 'O':
                self._other = bin(record)
            elif rtype == 'm':
                bits = record.split('\0', 1)
                mdstate = bits[1]
                if len(mdstate) != 1 or mdstate not in 'ums':
                    # the merge driver should be idempotent, so just rerun it
                    mdstate = 'u'

                self._readmergedriver = bits[0]
                self._mdstate = mdstate
            elif rtype in 'FDC':
                bits = record.split('\0')
                self._state[bits[0]] = bits[1:]
            elif rtype == 'f':
                filename, rawextras = record.split('\0', 1)
                extraparts = rawextras.split('\0')
                extras = {}
                i = 0
                while i < len(extraparts):
                    extras[extraparts[i]] = extraparts[i + 1]
                    i += 2

                self._stateextras[filename] = extras
            elif rtype == 'l':
                labels = record.split('\0', 2)
                self._labels = [l for l in labels if len(l) > 0]
            elif not rtype.islower():
                unsupported.add(rtype)
        self._results = {}
        self._dirty = False

        if unsupported:
            raise error.UnsupportedMergeRecords(unsupported)

    def _readrecords(self):
        """Read merge state from disk and return a list of record (TYPE, data)

        We read data from both v1 and v2 files and decide which one to use.

        V1 has been used by version prior to 2.9.1 and contains less data than
        v2. We read both versions and check if no data in v2 contradicts
        v1. If there is not contradiction we can safely assume that both v1
        and v2 were written at the same time and use the extract data in v2. If
        there is contradiction we ignore v2 content as we assume an old version
        of Mercurial has overwritten the mergestate file and left an old v2
        file around.

        returns list of record [(TYPE, data), ...]"""
        v1records = self._readrecordsv1()
        v2records = self._readrecordsv2()
        if self._v1v2match(v1records, v2records):
            return v2records
        else:
            # v1 file is newer than v2 file, use it
            # we have to infer the "other" changeset of the merge
            # we cannot do better than that with v1 of the format
            mctx = self._repo[None].parents()[-1]
            v1records.append(('O', mctx.hex()))
            # add place holder "other" file node information
            # nobody is using it yet so we do no need to fetch the data
            # if mctx was wrong `mctx[bits[-2]]` may fails.
            for idx, r in enumerate(v1records):
                if r[0] == 'F':
                    bits = r[1].split('\0')
                    bits.insert(-2, '')
                    v1records[idx] = (r[0], '\0'.join(bits))
            return v1records

    def _v1v2match(self, v1records, v2records):
        oldv2 = set() # old format version of v2 record
        for rec in v2records:
            if rec[0] == 'L':
                oldv2.add(rec)
            elif rec[0] == 'F':
                # drop the onode data (not contained in v1)
                oldv2.add(('F', _droponode(rec[1])))
        for rec in v1records:
            if rec not in oldv2:
                return False
        else:
            return True

    def _readrecordsv1(self):
        """read on disk merge state for version 1 file

        returns list of record [(TYPE, data), ...]

        Note: the "F" data from this file are one entry short
              (no "other file node" entry)
        """
        records = []
        try:
            f = self._repo.vfs(self.statepathv1)
            for i, l in enumerate(f):
                if i == 0:
                    records.append(('L', l[:-1]))
                else:
                    records.append(('F', l[:-1]))
            f.close()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
        return records

    def _readrecordsv2(self):
        """read on disk merge state for version 2 file

        This format is a list of arbitrary records of the form:

          [type][length][content]

        `type` is a single character, `length` is a 4 byte integer, and
        `content` is an arbitrary byte sequence of length `length`.

        Mercurial versions prior to 3.7 have a bug where if there are
        unsupported mandatory merge records, attempting to clear out the merge
        state with hg update --clean or similar aborts. The 't' record type
        works around that by writing out what those versions treat as an
        advisory record, but later versions interpret as special: the first
        character is the 'real' record type and everything onwards is the data.

        Returns list of records [(TYPE, data), ...]."""
        records = []
        try:
            f = self._repo.vfs(self.statepathv2)
            data = f.read()
            off = 0
            end = len(data)
            while off < end:
                rtype = data[off]
                off += 1
                length = _unpack('>I', data[off:(off + 4)])[0]
                off += 4
                record = data[off:(off + length)]
                off += length
                if rtype == 't':
                    rtype, record = record[0], record[1:]
                records.append((rtype, record))
            f.close()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
        return records

    @util.propertycache
    def mergedriver(self):
        # protect against the following:
        # - A configures a malicious merge driver in their hgrc, then
        #   pauses the merge
        # - A edits their hgrc to remove references to the merge driver
        # - A gives a copy of their entire repo, including .hg, to B
        # - B inspects .hgrc and finds it to be clean
        # - B then continues the merge and the malicious merge driver
        #  gets invoked
        configmergedriver = self._repo.ui.config('experimental', 'mergedriver')
        if (self._readmergedriver is not None
            and self._readmergedriver != configmergedriver):
            raise error.ConfigError(
                _("merge driver changed since merge started"),
                hint=_("revert merge driver change or abort merge"))

        return configmergedriver

    @util.propertycache
    def localctx(self):
        if self._local is None:
            raise RuntimeError("localctx accessed but self._local isn't set")
        return self._repo[self._local]

    @util.propertycache
    def otherctx(self):
        if self._other is None:
            raise RuntimeError("otherctx accessed but self._other isn't set")
        return self._repo[self._other]

    def active(self):
        """Whether mergestate is active.

        Returns True if there appears to be mergestate. This is a rough proxy
        for "is a merge in progress."
        """
        # Check local variables before looking at filesystem for performance
        # reasons.
        return bool(self._local) or bool(self._state) or \
               self._repo.vfs.exists(self.statepathv1) or \
               self._repo.vfs.exists(self.statepathv2)

    def commit(self):
        """Write current state on disk (if necessary)"""
        if self._dirty:
            records = self._makerecords()
            self._writerecords(records)
            self._dirty = False

    def _makerecords(self):
        records = []
        records.append(('L', hex(self._local)))
        records.append(('O', hex(self._other)))
        if self.mergedriver:
            records.append(('m', '\0'.join([
                self.mergedriver, self._mdstate])))
        for d, v in self._state.iteritems():
            if v[0] == 'd':
                records.append(('D', '\0'.join([d] + v)))
            # v[1] == local ('cd'), v[6] == other ('dc') -- not supported by
            # older versions of Mercurial
            elif v[1] == nullhex or v[6] == nullhex:
                records.append(('C', '\0'.join([d] + v)))
            else:
                records.append(('F', '\0'.join([d] + v)))
        for filename, extras in sorted(self._stateextras.iteritems()):
            rawextras = '\0'.join('%s\0%s' % (k, v) for k, v in
                                  extras.iteritems())
            records.append(('f', '%s\0%s' % (filename, rawextras)))
        if self._labels is not None:
            labels = '\0'.join(self._labels)
            records.append(('l', labels))
        return records

    def _writerecords(self, records):
        """Write current state on disk (both v1 and v2)"""
        self._writerecordsv1(records)
        self._writerecordsv2(records)

    def _writerecordsv1(self, records):
        """Write current state on disk in a version 1 file"""
        f = self._repo.vfs(self.statepathv1, 'w')
        irecords = iter(records)
        lrecords = irecords.next()
        assert lrecords[0] == 'L'
        f.write(hex(self._local) + '\n')
        for rtype, data in irecords:
            if rtype == 'F':
                f.write('%s\n' % _droponode(data))
        f.close()

    def _writerecordsv2(self, records):
        """Write current state on disk in a version 2 file

        See the docstring for _readrecordsv2 for why we use 't'."""
        # these are the records that all version 2 clients can read
        whitelist = 'LOF'
        f = self._repo.vfs(self.statepathv2, 'w')
        for key, data in records:
            assert len(key) == 1
            if key not in whitelist:
                key, data = 't', '%s%s' % (key, data)
            format = '>sI%is' % len(data)
            f.write(_pack(format, key, len(data), data))
        f.close()

    def add(self, fcl, fco, fca, fd):
        """add a new (potentially?) conflicting file the merge state
        fcl: file context for local,
        fco: file context for remote,
        fca: file context for ancestors,
        fd:  file path of the resulting merge.

        note: also write the local version to the `.hg/merge` directory.
        """
        if fcl.isabsent():
            hash = nullhex
        else:
            hash = util.sha1(fcl.path()).hexdigest()
            self._repo.vfs.write('merge/' + hash, fcl.data())
        self._state[fd] = ['u', hash, fcl.path(),
                           fca.path(), hex(fca.filenode()),
                           fco.path(), hex(fco.filenode()),
                           fcl.flags()]
        self._stateextras[fd] = { 'ancestorlinknode' : hex(fca.node()) }
        self._dirty = True

    def __contains__(self, dfile):
        return dfile in self._state

    def __getitem__(self, dfile):
        return self._state[dfile][0]

    def __iter__(self):
        return iter(sorted(self._state))

    def files(self):
        return self._state.keys()

    def mark(self, dfile, state):
        self._state[dfile][0] = state
        self._dirty = True

    def mdstate(self):
        return self._mdstate

    def unresolved(self):
        """Obtain the paths of unresolved files."""

        for f, entry in self._state.items():
            if entry[0] == 'u':
                yield f

    def driverresolved(self):
        """Obtain the paths of driver-resolved files."""

        for f, entry in self._state.items():
            if entry[0] == 'd':
                yield f

    def extras(self, filename):
        return self._stateextras.setdefault(filename, {})

    def _resolve(self, preresolve, dfile, wctx):
        """rerun merge process for file path `dfile`"""
        if self[dfile] in 'rd':
            return True, 0
        stateentry = self._state[dfile]
        state, hash, lfile, afile, anode, ofile, onode, flags = stateentry
        octx = self._repo[self._other]
        extras = self.extras(dfile)
        anccommitnode = extras.get('ancestorlinknode')
        if anccommitnode:
            actx = self._repo[anccommitnode]
        else:
            actx = None
        fcd = self._filectxorabsent(hash, wctx, dfile)
        fco = self._filectxorabsent(onode, octx, ofile)
        # TODO: move this to filectxorabsent
        fca = self._repo.filectx(afile, fileid=anode, changeid=actx)
        # "premerge" x flags
        flo = fco.flags()
        fla = fca.flags()
        if 'x' in flags + flo + fla and 'l' not in flags + flo + fla:
            if fca.node() == nullid:
                if preresolve:
                    self._repo.ui.warn(
                        _('warning: cannot merge flags for %s\n') % afile)
            elif flags == fla:
                flags = flo
        if preresolve:
            # restore local
            if hash != nullhex:
                f = self._repo.vfs('merge/' + hash)
                self._repo.wwrite(dfile, f.read(), flags)
                f.close()
            else:
                self._repo.wvfs.unlinkpath(dfile, ignoremissing=True)
            complete, r, deleted = filemerge.premerge(self._repo, self._local,
                                                      lfile, fcd, fco, fca,
                                                      labels=self._labels)
        else:
            complete, r, deleted = filemerge.filemerge(self._repo, self._local,
                                                       lfile, fcd, fco, fca,
                                                       labels=self._labels)
        if r is None:
            # no real conflict
            del self._state[dfile]
            self._stateextras.pop(dfile, None)
            self._dirty = True
        elif not r:
            self.mark(dfile, 'r')

        if complete:
            action = None
            if deleted:
                if fcd.isabsent():
                    # dc: local picked. Need to drop if present, which may
                    # happen on re-resolves.
                    action = 'f'
                else:
                    # cd: remote picked (or otherwise deleted)
                    action = 'r'
            else:
                if fcd.isabsent(): # dc: remote picked
                    action = 'g'
                elif fco.isabsent(): # cd: local picked
                    if dfile in self.localctx:
                        action = 'am'
                    else:
                        action = 'a'
                # else: regular merges (no action necessary)
            self._results[dfile] = r, action

        return complete, r

    def _filectxorabsent(self, hexnode, ctx, f):
        if hexnode == nullhex:
            return filemerge.absentfilectx(ctx, f)
        else:
            return ctx[f]

    def preresolve(self, dfile, wctx):
        """run premerge process for dfile

        Returns whether the merge is complete, and the exit code."""
        return self._resolve(True, dfile, wctx)

    def resolve(self, dfile, wctx):
        """run merge process (assuming premerge was run) for dfile

        Returns the exit code of the merge."""
        return self._resolve(False, dfile, wctx)[1]

    def counts(self):
        """return counts for updated, merged and removed files in this
        session"""
        updated, merged, removed = 0, 0, 0
        for r, action in self._results.itervalues():
            if r is None:
                updated += 1
            elif r == 0:
                if action == 'r':
                    removed += 1
                else:
                    merged += 1
        return updated, merged, removed

    def unresolvedcount(self):
        """get unresolved count for this merge (persistent)"""
        return len([True for f, entry in self._state.iteritems()
                    if entry[0] == 'u'])

    def actions(self):
        """return lists of actions to perform on the dirstate"""
        actions = {'r': [], 'f': [], 'a': [], 'am': [], 'g': []}
        for f, (r, action) in self._results.iteritems():
            if action is not None:
                actions[action].append((f, None, "merge result"))
        return actions

    def recordactions(self):
        """record remove/add/get actions in the dirstate"""
        branchmerge = self._repo.dirstate.p2() != nullid
        recordupdates(self._repo, self.actions(), branchmerge)

    def queueremove(self, f):
        """queues a file to be removed from the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, 'r'

    def queueadd(self, f):
        """queues a file to be added to the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, 'a'

    def queueget(self, f):
        """queues a file to be marked modified in the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, 'g'

def _getcheckunknownconfig(repo, section, name):
    config = repo.ui.config(section, name, default='abort')
    valid = ['abort', 'ignore', 'warn']
    if config not in valid:
        validstr = ', '.join(["'" + v + "'" for v in valid])
        raise error.ConfigError(_("%s.%s not valid "
                                  "('%s' is none of %s)")
                                % (section, name, config, validstr))
    return config

def _checkunknownfile(repo, wctx, mctx, f, f2=None):
    if f2 is None:
        f2 = f
    return (repo.wvfs.audit.check(f)
        and repo.wvfs.isfileorlink(f)
        and repo.dirstate.normalize(f) not in repo.dirstate
        and mctx[f2].cmp(wctx[f]))

def _checkunknownfiles(repo, wctx, mctx, force, actions, mergeforce):
    """
    Considers any actions that care about the presence of conflicting unknown
    files. For some actions, the result is to abort; for others, it is to
    choose a different action.
    """
    conflicts = set()
    warnconflicts = set()
    abortconflicts = set()
    unknownconfig = _getcheckunknownconfig(repo, 'merge', 'checkunknown')
    ignoredconfig = _getcheckunknownconfig(repo, 'merge', 'checkignored')
    if not force:
        def collectconflicts(conflicts, config):
            if config == 'abort':
                abortconflicts.update(conflicts)
            elif config == 'warn':
                warnconflicts.update(conflicts)

        for f, (m, args, msg) in actions.iteritems():
            if m in ('c', 'dc'):
                if _checkunknownfile(repo, wctx, mctx, f):
                    conflicts.add(f)
            elif m == 'dg':
                if _checkunknownfile(repo, wctx, mctx, f, args[0]):
                    conflicts.add(f)

        ignoredconflicts = set([c for c in conflicts
                                if repo.dirstate._ignore(c)])
        unknownconflicts = conflicts - ignoredconflicts
        collectconflicts(ignoredconflicts, ignoredconfig)
        collectconflicts(unknownconflicts, unknownconfig)
    else:
        for f, (m, args, msg) in actions.iteritems():
            if m == 'cm':
                fl2, anc = args
                different = _checkunknownfile(repo, wctx, mctx, f)
                if repo.dirstate._ignore(f):
                    config = ignoredconfig
                else:
                    config = unknownconfig

                # The behavior when force is True is described by this table:
                #  config  different  mergeforce  |    action    backup
                #    *         n          *       |      get        n
                #    *         y          y       |     merge       -
                #   abort      y          n       |     merge       -   (1)
                #   warn       y          n       |  warn + get     y
                #  ignore      y          n       |      get        y
                #
                # (1) this is probably the wrong behavior here -- we should
                #     probably abort, but some actions like rebases currently
                #     don't like an abort happening in the middle of
                #     merge.update.
                if not different:
                    actions[f] = ('g', (fl2, False), "remote created")
                elif mergeforce or config == 'abort':
                    actions[f] = ('m', (f, f, None, False, anc),
                                  "remote differs from untracked local")
                elif config == 'abort':
                    abortconflicts.add(f)
                else:
                    if config == 'warn':
                        warnconflicts.add(f)
                    actions[f] = ('g', (fl2, True), "remote created")

    for f in sorted(abortconflicts):
        repo.ui.warn(_("%s: untracked file differs\n") % f)
    if abortconflicts:
        raise error.Abort(_("untracked files in working directory "
                            "differ from files in requested revision"))

    for f in sorted(warnconflicts):
        repo.ui.warn(_("%s: replacing untracked file\n") % f)

    for f, (m, args, msg) in actions.iteritems():
        backup = f in conflicts
        if m == 'c':
            flags, = args
            actions[f] = ('g', (flags, backup), msg)

def _forgetremoved(wctx, mctx, branchmerge):
    """
    Forget removed files

    If we're jumping between revisions (as opposed to merging), and if
    neither the working directory nor the target rev has the file,
    then we need to remove it from the dirstate, to prevent the
    dirstate from listing the file when it is no longer in the
    manifest.

    If we're merging, and the other revision has removed a file
    that is not present in the working directory, we need to mark it
    as removed.
    """

    actions = {}
    m = 'f'
    if branchmerge:
        m = 'r'
    for f in wctx.deleted():
        if f not in mctx:
            actions[f] = m, None, "forget deleted"

    if not branchmerge:
        for f in wctx.removed():
            if f not in mctx:
                actions[f] = 'f', None, "forget removed"

    return actions

def _checkcollision(repo, wmf, actions):
    # build provisional merged manifest up
    pmmf = set(wmf)

    if actions:
        # k, dr, e and rd are no-op
        for m in 'a', 'am', 'f', 'g', 'cd', 'dc':
            for f, args, msg in actions[m]:
                pmmf.add(f)
        for f, args, msg in actions['r']:
            pmmf.discard(f)
        for f, args, msg in actions['dm']:
            f2, flags = args
            pmmf.discard(f2)
            pmmf.add(f)
        for f, args, msg in actions['dg']:
            pmmf.add(f)
        for f, args, msg in actions['m']:
            f1, f2, fa, move, anc = args
            if move:
                pmmf.discard(f1)
            pmmf.add(f)

    # check case-folding collision in provisional merged manifest
    foldmap = {}
    for f in sorted(pmmf):
        fold = util.normcase(f)
        if fold in foldmap:
            raise error.Abort(_("case-folding collision between %s and %s")
                             % (f, foldmap[fold]))
        foldmap[fold] = f

    # check case-folding of directories
    foldprefix = unfoldprefix = lastfull = ''
    for fold, f in sorted(foldmap.items()):
        if fold.startswith(foldprefix) and not f.startswith(unfoldprefix):
            # the folded prefix matches but actual casing is different
            raise error.Abort(_("case-folding collision between "
                                "%s and directory of %s") % (lastfull, f))
        foldprefix = fold + '/'
        unfoldprefix = f + '/'
        lastfull = f

def driverpreprocess(repo, ms, wctx, labels=None):
    """run the preprocess step of the merge driver, if any

    This is currently not implemented -- it's an extension point."""
    return True

def driverconclude(repo, ms, wctx, labels=None):
    """run the conclude step of the merge driver, if any

    This is currently not implemented -- it's an extension point."""
    return True

def manifestmerge(repo, wctx, p2, pa, branchmerge, force, matcher,
                  acceptremote, followcopies):
    """
    Merge p1 and p2 with ancestor pa and generate merge action list

    branchmerge and force are as passed in to update
    matcher = matcher to filter file lists
    acceptremote = accept the incoming changes without prompting
    """
    if matcher is not None and matcher.always():
        matcher = None

    copy, movewithdir, diverge, renamedelete = {}, {}, {}, {}

    # manifests fetched in order are going to be faster, so prime the caches
    [x.manifest() for x in
     sorted(wctx.parents() + [p2, pa], key=lambda x: x.rev())]

    if followcopies:
        ret = copies.mergecopies(repo, wctx, p2, pa)
        copy, movewithdir, diverge, renamedelete = ret

    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(" branchmerge: %s, force: %s, partial: %s\n"
                  % (bool(branchmerge), bool(force), bool(matcher)))
    repo.ui.debug(" ancestor: %s, local: %s, remote: %s\n" % (pa, wctx, p2))

    m1, m2, ma = wctx.manifest(), p2.manifest(), pa.manifest()
    copied = set(copy.values())
    copied.update(movewithdir.values())

    if '.hgsubstate' in m1:
        # check whether sub state is modified
        if any(wctx.sub(s).dirty() for s in wctx.substate):
            m1['.hgsubstate'] += '+'

    # Compare manifests
    if matcher is not None:
        m1 = m1.matches(matcher)
        m2 = m2.matches(matcher)
    diff = m1.diff(m2)

    actions = {}
    for f, ((n1, fl1), (n2, fl2)) in diff.iteritems():
        if n1 and n2: # file exists on both local and remote side
            if f not in ma:
                fa = copy.get(f, None)
                if fa is not None:
                    actions[f] = ('m', (f, f, fa, False, pa.node()),
                                  "both renamed from " + fa)
                else:
                    actions[f] = ('m', (f, f, None, False, pa.node()),
                                  "both created")
            else:
                a = ma[f]
                fla = ma.flags(f)
                nol = 'l' not in fl1 + fl2 + fla
                if n2 == a and fl2 == fla:
                    actions[f] = ('k' , (), "remote unchanged")
                elif n1 == a and fl1 == fla: # local unchanged - use remote
                    if n1 == n2: # optimization: keep local content
                        actions[f] = ('e', (fl2,), "update permissions")
                    else:
                        actions[f] = ('g', (fl2, False), "remote is newer")
                elif nol and n2 == a: # remote only changed 'x'
                    actions[f] = ('e', (fl2,), "update permissions")
                elif nol and n1 == a: # local only changed 'x'
                    actions[f] = ('g', (fl1, False), "remote is newer")
                else: # both changed something
                    actions[f] = ('m', (f, f, f, False, pa.node()),
                                   "versions differ")
        elif n1: # file exists only on local side
            if f in copied:
                pass # we'll deal with it on m2 side
            elif f in movewithdir: # directory rename, move local
                f2 = movewithdir[f]
                if f2 in m2:
                    actions[f2] = ('m', (f, f2, None, True, pa.node()),
                                   "remote directory rename, both created")
                else:
                    actions[f2] = ('dm', (f, fl1),
                                   "remote directory rename - move from " + f)
            elif f in copy:
                f2 = copy[f]
                actions[f] = ('m', (f, f2, f2, False, pa.node()),
                              "local copied/moved from " + f2)
            elif f in ma: # clean, a different, no remote
                if n1 != ma[f]:
                    if acceptremote:
                        actions[f] = ('r', None, "remote delete")
                    else:
                        actions[f] = ('cd', (f, None, f, False, pa.node()),
                                      "prompt changed/deleted")
                elif n1[20:] == 'a':
                    # This extra 'a' is added by working copy manifest to mark
                    # the file as locally added. We should forget it instead of
                    # deleting it.
                    actions[f] = ('f', None, "remote deleted")
                else:
                    actions[f] = ('r', None, "other deleted")
        elif n2: # file exists only on remote side
            if f in copied:
                pass # we'll deal with it on m1 side
            elif f in movewithdir:
                f2 = movewithdir[f]
                if f2 in m1:
                    actions[f2] = ('m', (f2, f, None, False, pa.node()),
                                   "local directory rename, both created")
                else:
                    actions[f2] = ('dg', (f, fl2),
                                   "local directory rename - get from " + f)
            elif f in copy:
                f2 = copy[f]
                if f2 in m2:
                    actions[f] = ('m', (f2, f, f2, False, pa.node()),
                                  "remote copied from " + f2)
                else:
                    actions[f] = ('m', (f2, f, f2, True, pa.node()),
                                  "remote moved from " + f2)
            elif f not in ma:
                # local unknown, remote created: the logic is described by the
                # following table:
                #
                # force  branchmerge  different  |  action
                #   n         *           *      |   create
                #   y         n           *      |   create
                #   y         y           n      |   create
                #   y         y           y      |   merge
                #
                # Checking whether the files are different is expensive, so we
                # don't do that when we can avoid it.
                if not force:
                    actions[f] = ('c', (fl2,), "remote created")
                elif not branchmerge:
                    actions[f] = ('c', (fl2,), "remote created")
                else:
                    actions[f] = ('cm', (fl2, pa.node()),
                                  "remote created, get or merge")
            elif n2 != ma[f]:
                if acceptremote:
                    actions[f] = ('c', (fl2,), "remote recreating")
                else:
                    actions[f] = ('dc', (None, f, f, False, pa.node()),
                                  "prompt deleted/changed")

    return actions, diverge, renamedelete

def _resolvetrivial(repo, wctx, mctx, ancestor, actions):
    """Resolves false conflicts where the nodeid changed but the content
       remained the same."""

    for f, (m, args, msg) in actions.items():
        if m == 'cd' and f in ancestor and not wctx[f].cmp(ancestor[f]):
            # local did change but ended up with same content
            actions[f] = 'r', None, "prompt same"
        elif m == 'dc' and f in ancestor and not mctx[f].cmp(ancestor[f]):
            # remote did change but ended up with same content
            del actions[f] # don't get = keep local deleted

def calculateupdates(repo, wctx, mctx, ancestors, branchmerge, force,
                     acceptremote, followcopies, matcher=None,
                     mergeforce=False):
    "Calculate the actions needed to merge mctx into wctx using ancestors"
    if len(ancestors) == 1: # default
        actions, diverge, renamedelete = manifestmerge(
            repo, wctx, mctx, ancestors[0], branchmerge, force, matcher,
            acceptremote, followcopies)
        _checkunknownfiles(repo, wctx, mctx, force, actions, mergeforce)

    else: # only when merge.preferancestor=* - the default
        repo.ui.note(
            _("note: merging %s and %s using bids from ancestors %s\n") %
            (wctx, mctx, _(' and ').join(str(anc) for anc in ancestors)))

        # Call for bids
        fbids = {} # mapping filename to bids (action method to list af actions)
        diverge, renamedelete = None, None
        for ancestor in ancestors:
            repo.ui.note(_('\ncalculating bids for ancestor %s\n') % ancestor)
            actions, diverge1, renamedelete1 = manifestmerge(
                repo, wctx, mctx, ancestor, branchmerge, force, matcher,
                acceptremote, followcopies)
            _checkunknownfiles(repo, wctx, mctx, force, actions, mergeforce)

            # Track the shortest set of warning on the theory that bid
            # merge will correctly incorporate more information
            if diverge is None or len(diverge1) < len(diverge):
                diverge = diverge1
            if renamedelete is None or len(renamedelete) < len(renamedelete1):
                renamedelete = renamedelete1

            for f, a in sorted(actions.iteritems()):
                m, args, msg = a
                repo.ui.debug(' %s: %s -> %s\n' % (f, msg, m))
                if f in fbids:
                    d = fbids[f]
                    if m in d:
                        d[m].append(a)
                    else:
                        d[m] = [a]
                else:
                    fbids[f] = {m: [a]}

        # Pick the best bid for each file
        repo.ui.note(_('\nauction for merging merge bids\n'))
        actions = {}
        for f, bids in sorted(fbids.items()):
            # bids is a mapping from action method to list af actions
            # Consensus?
            if len(bids) == 1: # all bids are the same kind of method
                m, l = bids.items()[0]
                if all(a == l[0] for a in l[1:]): # len(bids) is > 1
                    repo.ui.note(" %s: consensus for %s\n" % (f, m))
                    actions[f] = l[0]
                    continue
            # If keep is an option, just do it.
            if 'k' in bids:
                repo.ui.note(" %s: picking 'keep' action\n" % f)
                actions[f] = bids['k'][0]
                continue
            # If there are gets and they all agree [how could they not?], do it.
            if 'g' in bids:
                ga0 = bids['g'][0]
                if all(a == ga0 for a in bids['g'][1:]):
                    repo.ui.note(" %s: picking 'get' action\n" % f)
                    actions[f] = ga0
                    continue
            # TODO: Consider other simple actions such as mode changes
            # Handle inefficient democrazy.
            repo.ui.note(_(' %s: multiple bids for merge action:\n') % f)
            for m, l in sorted(bids.items()):
                for _f, args, msg in l:
                    repo.ui.note('  %s -> %s\n' % (msg, m))
            # Pick random action. TODO: Instead, prompt user when resolving
            m, l = bids.items()[0]
            repo.ui.warn(_(' %s: ambiguous merge - picked %s action\n') %
                         (f, m))
            actions[f] = l[0]
            continue
        repo.ui.note(_('end of auction\n\n'))

    _resolvetrivial(repo, wctx, mctx, ancestors[0], actions)

    if wctx.rev() is None:
        fractions = _forgetremoved(wctx, mctx, branchmerge)
        actions.update(fractions)

    return actions, diverge, renamedelete

def batchremove(repo, actions):
    """apply removes to the working directory

    yields tuples for progress updates
    """
    verbose = repo.ui.verbose
    unlink = util.unlinkpath
    wjoin = repo.wjoin
    audit = repo.wvfs.audit
    i = 0
    for f, args, msg in actions:
        repo.ui.debug(" %s: %s -> r\n" % (f, msg))
        if verbose:
            repo.ui.note(_("removing %s\n") % f)
        audit(f)
        try:
            unlink(wjoin(f), ignoremissing=True)
        except OSError as inst:
            repo.ui.warn(_("update failed to remove %s: %s!\n") %
                         (f, inst.strerror))
        if i == 100:
            yield i, f
            i = 0
        i += 1
    if i > 0:
        yield i, f

def batchget(repo, mctx, actions):
    """apply gets to the working directory

    mctx is the context to get from

    yields tuples for progress updates
    """
    verbose = repo.ui.verbose
    fctx = mctx.filectx
    wwrite = repo.wwrite
    ui = repo.ui
    i = 0
    with repo.wvfs.backgroundclosing(ui, expectedcount=len(actions)):
        for f, (flags, backup), msg in actions:
            repo.ui.debug(" %s: %s -> g\n" % (f, msg))
            if verbose:
                repo.ui.note(_("getting %s\n") % f)

            if backup:
                absf = repo.wjoin(f)
                orig = scmutil.origpath(ui, repo, absf)
                try:
                    # TODO Mercurial has always aborted if an untracked
                    # directory is replaced by a tracked file, or generally
                    # with file/directory merges. This needs to be sorted out.
                    if repo.wvfs.isfileorlink(f):
                        util.rename(absf, orig)
                except OSError as e:
                    if e.errno != errno.ENOENT:
                        raise

            wwrite(f, fctx(f).data(), flags, backgroundclose=True)
            if i == 100:
                yield i, f
                i = 0
            i += 1
    if i > 0:
        yield i, f

def applyupdates(repo, actions, wctx, mctx, overwrite, labels=None):
    """apply the merge action list to the working directory

    wctx is the working copy context
    mctx is the context to be merged into the working copy

    Return a tuple of counts (updated, merged, removed, unresolved) that
    describes how many files were affected by the update.
    """

    updated, merged, removed = 0, 0, 0
    ms = mergestate.clean(repo, wctx.p1().node(), mctx.node(), labels)
    moves = []
    for m, l in actions.items():
        l.sort()

    # 'cd' and 'dc' actions are treated like other merge conflicts
    mergeactions = sorted(actions['cd'])
    mergeactions.extend(sorted(actions['dc']))
    mergeactions.extend(actions['m'])
    for f, args, msg in mergeactions:
        f1, f2, fa, move, anc = args
        if f == '.hgsubstate': # merged internally
            continue
        if f1 is None:
            fcl = filemerge.absentfilectx(wctx, fa)
        else:
            repo.ui.debug(" preserving %s for resolve of %s\n" % (f1, f))
            fcl = wctx[f1]
        if f2 is None:
            fco = filemerge.absentfilectx(mctx, fa)
        else:
            fco = mctx[f2]
        actx = repo[anc]
        if fa in actx:
            fca = actx[fa]
        else:
            # TODO: move to absentfilectx
            fca = repo.filectx(f1, fileid=nullrev)
        ms.add(fcl, fco, fca, f)
        if f1 != f and move:
            moves.append(f1)

    audit = repo.wvfs.audit
    _updating = _('updating')
    _files = _('files')
    progress = repo.ui.progress

    # remove renamed files after safely stored
    for f in moves:
        if os.path.lexists(repo.wjoin(f)):
            repo.ui.debug("removing %s\n" % f)
            audit(f)
            util.unlinkpath(repo.wjoin(f))

    numupdates = sum(len(l) for m, l in actions.items() if m != 'k')

    if [a for a in actions['r'] if a[0] == '.hgsubstate']:
        subrepo.submerge(repo, wctx, mctx, wctx, overwrite)

    # remove in parallel (must come first)
    z = 0
    prog = worker.worker(repo.ui, 0.001, batchremove, (repo,), actions['r'])
    for i, item in prog:
        z += i
        progress(_updating, z, item=item, total=numupdates, unit=_files)
    removed = len(actions['r'])

    # get in parallel
    prog = worker.worker(repo.ui, 0.001, batchget, (repo, mctx), actions['g'])
    for i, item in prog:
        z += i
        progress(_updating, z, item=item, total=numupdates, unit=_files)
    updated = len(actions['g'])

    if [a for a in actions['g'] if a[0] == '.hgsubstate']:
        subrepo.submerge(repo, wctx, mctx, wctx, overwrite)

    # forget (manifest only, just log it) (must come first)
    for f, args, msg in actions['f']:
        repo.ui.debug(" %s: %s -> f\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)

    # re-add (manifest only, just log it)
    for f, args, msg in actions['a']:
        repo.ui.debug(" %s: %s -> a\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)

    # re-add/mark as modified (manifest only, just log it)
    for f, args, msg in actions['am']:
        repo.ui.debug(" %s: %s -> am\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)

    # keep (noop, just log it)
    for f, args, msg in actions['k']:
        repo.ui.debug(" %s: %s -> k\n" % (f, msg))
        # no progress

    # directory rename, move local
    for f, args, msg in actions['dm']:
        repo.ui.debug(" %s: %s -> dm\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        f0, flags = args
        repo.ui.note(_("moving %s to %s\n") % (f0, f))
        audit(f)
        repo.wwrite(f, wctx.filectx(f0).data(), flags)
        util.unlinkpath(repo.wjoin(f0))
        updated += 1

    # local directory rename, get
    for f, args, msg in actions['dg']:
        repo.ui.debug(" %s: %s -> dg\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        f0, flags = args
        repo.ui.note(_("getting %s to %s\n") % (f0, f))
        repo.wwrite(f, mctx.filectx(f0).data(), flags)
        updated += 1

    # exec
    for f, args, msg in actions['e']:
        repo.ui.debug(" %s: %s -> e\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        flags, = args
        audit(f)
        util.setflags(repo.wjoin(f), 'l' in flags, 'x' in flags)
        updated += 1

    # the ordering is important here -- ms.mergedriver will raise if the merge
    # driver has changed, and we want to be able to bypass it when overwrite is
    # True
    usemergedriver = not overwrite and mergeactions and ms.mergedriver

    if usemergedriver:
        ms.commit()
        proceed = driverpreprocess(repo, ms, wctx, labels=labels)
        # the driver might leave some files unresolved
        unresolvedf = set(ms.unresolved())
        if not proceed:
            # XXX setting unresolved to at least 1 is a hack to make sure we
            # error out
            return updated, merged, removed, max(len(unresolvedf), 1)
        newactions = []
        for f, args, msg in mergeactions:
            if f in unresolvedf:
                newactions.append((f, args, msg))
        mergeactions = newactions

    # premerge
    tocomplete = []
    for f, args, msg in mergeactions:
        repo.ui.debug(" %s: %s -> m (premerge)\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        if f == '.hgsubstate': # subrepo states need updating
            subrepo.submerge(repo, wctx, mctx, wctx.ancestor(mctx),
                             overwrite)
            continue
        audit(f)
        complete, r = ms.preresolve(f, wctx)
        if not complete:
            numupdates += 1
            tocomplete.append((f, args, msg))

    # merge
    for f, args, msg in tocomplete:
        repo.ui.debug(" %s: %s -> m (merge)\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        ms.resolve(f, wctx)

    ms.commit()

    unresolved = ms.unresolvedcount()

    if usemergedriver and not unresolved and ms.mdstate() != 's':
        if not driverconclude(repo, ms, wctx, labels=labels):
            # XXX setting unresolved to at least 1 is a hack to make sure we
            # error out
            unresolved = max(unresolved, 1)

        ms.commit()

    msupdated, msmerged, msremoved = ms.counts()
    updated += msupdated
    merged += msmerged
    removed += msremoved

    extraactions = ms.actions()
    for k, acts in extraactions.iteritems():
        actions[k].extend(acts)

    progress(_updating, None, total=numupdates, unit=_files)

    return updated, merged, removed, unresolved

def recordupdates(repo, actions, branchmerge):
    "record merge actions to the dirstate"
    # remove (must come first)
    for f, args, msg in actions.get('r', []):
        if branchmerge:
            repo.dirstate.remove(f)
        else:
            repo.dirstate.drop(f)

    # forget (must come first)
    for f, args, msg in actions.get('f', []):
        repo.dirstate.drop(f)

    # re-add
    for f, args, msg in actions.get('a', []):
        repo.dirstate.add(f)

    # re-add/mark as modified
    for f, args, msg in actions.get('am', []):
        if branchmerge:
            repo.dirstate.normallookup(f)
        else:
            repo.dirstate.add(f)

    # exec change
    for f, args, msg in actions.get('e', []):
        repo.dirstate.normallookup(f)

    # keep
    for f, args, msg in actions.get('k', []):
        pass

    # get
    for f, args, msg in actions.get('g', []):
        if branchmerge:
            repo.dirstate.otherparent(f)
        else:
            repo.dirstate.normal(f)

    # merge
    for f, args, msg in actions.get('m', []):
        f1, f2, fa, move, anc = args
        if branchmerge:
            # We've done a branch merge, mark this file as merged
            # so that we properly record the merger later
            repo.dirstate.merge(f)
            if f1 != f2: # copy/rename
                if move:
                    repo.dirstate.remove(f1)
                if f1 != f:
                    repo.dirstate.copy(f1, f)
                else:
                    repo.dirstate.copy(f2, f)
        else:
            # We've update-merged a locally modified file, so
            # we set the dirstate to emulate a normal checkout
            # of that file some time in the past. Thus our
            # merge will appear as a normal local file
            # modification.
            if f2 == f: # file not locally copied/moved
                repo.dirstate.normallookup(f)
            if move:
                repo.dirstate.drop(f1)

    # directory rename, move local
    for f, args, msg in actions.get('dm', []):
        f0, flag = args
        if branchmerge:
            repo.dirstate.add(f)
            repo.dirstate.remove(f0)
            repo.dirstate.copy(f0, f)
        else:
            repo.dirstate.normal(f)
            repo.dirstate.drop(f0)

    # directory rename, get
    for f, args, msg in actions.get('dg', []):
        f0, flag = args
        if branchmerge:
            repo.dirstate.add(f)
            repo.dirstate.copy(f0, f)
        else:
            repo.dirstate.normal(f)

def update(repo, node, branchmerge, force, ancestor=None,
           mergeancestor=False, labels=None, matcher=None, mergeforce=False):
    """
    Perform a merge between the working directory and the given node

    node = the node to update to, or None if unspecified
    branchmerge = whether to merge between branches
    force = whether to force branch merging or file overwriting
    matcher = a matcher to filter file lists (dirstate not updated)
    mergeancestor = whether it is merging with an ancestor. If true,
      we should accept the incoming changes for any prompts that occur.
      If false, merging with an ancestor (fast-forward) is only allowed
      between different named branches. This flag is used by rebase extension
      as a temporary fix and should be avoided in general.
    labels = labels to use for base, local and other
    mergeforce = whether the merge was run with 'merge --force' (deprecated): if
      this is True, then 'force' should be True as well.

    The table below shows all the behaviors of the update command
    given the -c and -C or no options, whether the working directory
    is dirty, whether a revision is specified, and the relationship of
    the parent rev to the target rev (linear, on the same named
    branch, or on another named branch).

    This logic is tested by test-update-branches.t.

    -c  -C  dirty  rev  |  linear      same    cross
     n   n    n     n   |    ok        (1)       x
     n   n    n     y   |    ok        ok       ok
     n   n    y     n   |   merge      (2)      (2)
     n   n    y     y   |   merge      (3)      (3)
     n   y    *     *   |   discard   discard   discard
     y   n    y     *   |    (4)       (4)      (4)
     y   n    n     *   |    ok        ok       ok
     y   y    *     *   |    (5)       (5)      (5)

    x = can't happen
    * = don't-care
    1 = abort: not a linear update (merge or update --check to force update)
    2 = abort: uncommitted changes (commit and merge, or update --clean to
                 discard changes)
    3 = abort: uncommitted changes (commit or update --clean to discard changes)
    4 = abort: uncommitted changes (checked in commands.py)
    5 = incompatible options (checked in commands.py)

    Return the same tuple as applyupdates().
    """

    onode = node
    # If we're doing a partial update, we need to skip updating
    # the dirstate, so make a note of any partial-ness to the
    # update here.
    if matcher is None or matcher.always():
        partial = False
    else:
        partial = True
    with repo.wlock():
        wc = repo[None]
        pl = wc.parents()
        p1 = pl[0]
        pas = [None]
        if ancestor is not None:
            pas = [repo[ancestor]]

        if node is None:
            if (repo.ui.configbool('devel', 'all-warnings')
                    or repo.ui.configbool('devel', 'oldapi')):
                repo.ui.develwarn('update with no target')
            rev, _mark, _act = destutil.destupdate(repo)
            node = repo[rev].node()

        overwrite = force and not branchmerge

        p2 = repo[node]
        if pas[0] is None:
            if repo.ui.configlist('merge', 'preferancestor', ['*']) == ['*']:
                cahs = repo.changelog.commonancestorsheads(p1.node(), p2.node())
                pas = [repo[anc] for anc in (sorted(cahs) or [nullid])]
            else:
                pas = [p1.ancestor(p2, warn=branchmerge)]

        fp1, fp2, xp1, xp2 = p1.node(), p2.node(), str(p1), str(p2)

        ### check phase
        if not overwrite:
            if len(pl) > 1:
                raise error.Abort(_("outstanding uncommitted merge"))
            ms = mergestate.read(repo)
            if list(ms.unresolved()):
                raise error.Abort(_("outstanding merge conflicts"))
        if branchmerge:
            if pas == [p2]:
                raise error.Abort(_("merging with a working directory ancestor"
                                   " has no effect"))
            elif pas == [p1]:
                if not mergeancestor and p1.branch() == p2.branch():
                    raise error.Abort(_("nothing to merge"),
                                     hint=_("use 'hg update' "
                                            "or check 'hg heads'"))
            if not force and (wc.files() or wc.deleted()):
                raise error.Abort(_("uncommitted changes"),
                                 hint=_("use 'hg status' to list changes"))
            for s in sorted(wc.substate):
                wc.sub(s).bailifchanged()

        elif not overwrite:
            if p1 == p2: # no-op update
                # call the hooks and exit early
                repo.hook('preupdate', throw=True, parent1=xp2, parent2='')
                repo.hook('update', parent1=xp2, parent2='', error=0)
                return 0, 0, 0, 0

            if pas not in ([p1], [p2]):  # nonlinear
                dirty = wc.dirty(missing=True)
                if dirty or onode is None:
                    # Branching is a bit strange to ensure we do the minimal
                    # amount of call to obsolete.background.
                    foreground = obsolete.foreground(repo, [p1.node()])
                    # note: the <node> variable contains a random identifier
                    if repo[node].node() in foreground:
                        pas = [p1]  # allow updating to successors
                    elif dirty:
                        msg = _("uncommitted changes")
                        if onode is None:
                            hint = _("commit and merge, or update --clean to"
                                     " discard changes")
                        else:
                            hint = _("commit or update --clean to discard"
                                     " changes")
                        raise error.Abort(msg, hint=hint)
                    else:  # node is none
                        msg = _("not a linear update")
                        hint = _("merge or update --check to force update")
                        raise error.Abort(msg, hint=hint)
                else:
                    # Allow jumping branches if clean and specific rev given
                    pas = [p1]

        # deprecated config: merge.followcopies
        followcopies = False
        if overwrite:
            pas = [wc]
        elif pas == [p2]: # backwards
            pas = [wc.p1()]
        elif not branchmerge and not wc.dirty(missing=True):
            pass
        elif pas[0] and repo.ui.configbool('merge', 'followcopies', True):
            followcopies = True

        ### calculate phase
        actionbyfile, diverge, renamedelete = calculateupdates(
            repo, wc, p2, pas, branchmerge, force, mergeancestor,
            followcopies, matcher=matcher, mergeforce=mergeforce)

        # Prompt and create actions. Most of this is in the resolve phase
        # already, but we can't handle .hgsubstate in filemerge or
        # subrepo.submerge yet so we have to keep prompting for it.
        if '.hgsubstate' in actionbyfile:
            f = '.hgsubstate'
            m, args, msg = actionbyfile[f]
            if m == 'cd':
                if repo.ui.promptchoice(
                    _("local changed %s which remote deleted\n"
                      "use (c)hanged version or (d)elete?"
                      "$$ &Changed $$ &Delete") % f, 0):
                    actionbyfile[f] = ('r', None, "prompt delete")
                elif f in p1:
                    actionbyfile[f] = ('am', None, "prompt keep")
                else:
                    actionbyfile[f] = ('a', None, "prompt keep")
            elif m == 'dc':
                f1, f2, fa, move, anc = args
                flags = p2[f2].flags()
                if repo.ui.promptchoice(
                    _("remote changed %s which local deleted\n"
                      "use (c)hanged version or leave (d)eleted?"
                      "$$ &Changed $$ &Deleted") % f, 0) == 0:
                    actionbyfile[f] = ('g', (flags, False), "prompt recreating")
                else:
                    del actionbyfile[f]

        # Convert to dictionary-of-lists format
        actions = dict((m, []) for m in 'a am f g cd dc r dm dg m e k'.split())
        for f, (m, args, msg) in actionbyfile.iteritems():
            if m not in actions:
                actions[m] = []
            actions[m].append((f, args, msg))

        if not util.checkcase(repo.path):
            # check collision between files only in p2 for clean update
            if (not branchmerge and
                (force or not wc.dirty(missing=True, branch=False))):
                _checkcollision(repo, p2.manifest(), None)
            else:
                _checkcollision(repo, wc.manifest(), actions)

        # divergent renames
        for f, fl in sorted(diverge.iteritems()):
            repo.ui.warn(_("note: possible conflict - %s was renamed "
                           "multiple times to:\n") % f)
            for nf in fl:
                repo.ui.warn(" %s\n" % nf)

        # rename and delete
        for f, fl in sorted(renamedelete.iteritems()):
            repo.ui.warn(_("note: possible conflict - %s was deleted "
                           "and renamed to:\n") % f)
            for nf in fl:
                repo.ui.warn(" %s\n" % nf)

        ### apply phase
        if not branchmerge: # just jump to the new rev
            fp1, fp2, xp1, xp2 = fp2, nullid, xp2, ''
        if not partial:
            repo.hook('preupdate', throw=True, parent1=xp1, parent2=xp2)
            # note that we're in the middle of an update
            repo.vfs.write('updatestate', p2.hex())

        stats = applyupdates(repo, actions, wc, p2, overwrite, labels=labels)

        if not partial:
            repo.dirstate.beginparentchange()
            repo.setparents(fp1, fp2)
            recordupdates(repo, actions, branchmerge)
            # update completed, clear state
            util.unlink(repo.join('updatestate'))

            if not branchmerge:
                repo.dirstate.setbranch(p2.branch())
            repo.dirstate.endparentchange()

    if not partial:
        repo.hook('update', parent1=xp1, parent2=xp2, error=stats[3])
    return stats

def graft(repo, ctx, pctx, labels, keepparent=False):
    """Do a graft-like merge.

    This is a merge where the merge ancestor is chosen such that one
    or more changesets are grafted onto the current changeset. In
    addition to the merge, this fixes up the dirstate to include only
    a single parent (if keepparent is False) and tries to duplicate any
    renames/copies appropriately.

    ctx - changeset to rebase
    pctx - merge base, usually ctx.p1()
    labels - merge labels eg ['local', 'graft']
    keepparent - keep second parent if any

    """
    # If we're grafting a descendant onto an ancestor, be sure to pass
    # mergeancestor=True to update. This does two things: 1) allows the merge if
    # the destination is the same as the parent of the ctx (so we can use graft
    # to copy commits), and 2) informs update that the incoming changes are
    # newer than the destination so it doesn't prompt about "remote changed foo
    # which local deleted".
    mergeancestor = repo.changelog.isancestor(repo['.'].node(), ctx.node())

    stats = update(repo, ctx.node(), True, True, pctx.node(),
                   mergeancestor=mergeancestor, labels=labels)

    pother = nullid
    parents = ctx.parents()
    if keepparent and len(parents) == 2 and pctx in parents:
        parents.remove(pctx)
        pother = parents[0].node()

    repo.dirstate.beginparentchange()
    repo.setparents(repo['.'].node(), pother)
    repo.dirstate.write(repo.currenttransaction())
    # fix up dirstate for copies and renames
    copies.duplicatecopies(repo, ctx.rev(), pctx.rev())
    repo.dirstate.endparentchange()
    return stats

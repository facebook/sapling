# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import struct

from node import nullid, nullrev, hex, bin
from i18n import _
from mercurial import obsolete
import error as errormod, util, filemerge, copies, subrepo, worker
import errno, os, shutil

_pack = struct.pack
_unpack = struct.unpack

def _droponode(data):
    # used for compatibility for v1
    bits = data.split('\0')
    bits = bits[:-2] + bits[-1:]
    return '\0'.join(bits)

class mergestate(object):
    '''track 3-way merge state of individual files

    it is stored on disk when needed. Two file are used, one with an old
    format, one with a new format. Both contains similar data, but the new
    format can store new kind of field.

    Current new format is a list of arbitrary record of the form:

        [type][length][content]

    Type is a single character, length is a 4 bytes integer, content is an
    arbitrary suites of bytes of length `length`.

    Type should be a letter. Capital letter are mandatory record, Mercurial
    should abort if they are unknown. lower case record can be safely ignored.

    Currently known record:

    L: the node of the "local" part of the merge (hexified version)
    O: the node of the "other" part of the merge (hexified version)
    F: a file to be merged entry
    '''
    statepathv1 = 'merge/state'
    statepathv2 = 'merge/state2'

    def __init__(self, repo):
        self._repo = repo
        self._dirty = False
        self._read()

    def reset(self, node=None, other=None):
        self._state = {}
        self._local = None
        self._other = None
        if node:
            self._local = node
            self._other = other
        shutil.rmtree(self._repo.join('merge'), True)
        self._dirty = False

    def _read(self):
        """Analyse each record content to restore a serialized state from disk

        This function process "record" entry produced by the de-serialization
        of on disk file.
        """
        self._state = {}
        self._local = None
        self._other = None
        records = self._readrecords()
        for rtype, record in records:
            if rtype == 'L':
                self._local = bin(record)
            elif rtype == 'O':
                self._other = bin(record)
            elif rtype == 'F':
                bits = record.split('\0')
                self._state[bits[0]] = bits[1:]
            elif not rtype.islower():
                raise util.Abort(_('unsupported merge state record: %s')
                                   % rtype)
        self._dirty = False

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
        oldv2 = set() # old format version of v2 record
        for rec in v2records:
            if rec[0] == 'L':
                oldv2.add(rec)
            elif rec[0] == 'F':
                # drop the onode data (not contained in v1)
                oldv2.add(('F', _droponode(rec[1])))
        for rec in v1records:
            if rec not in oldv2:
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
        else:
            return v2records

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

        returns list of record [(TYPE, data), ...]
        """
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
                records.append((rtype, record))
            f.close()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
        return records

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
            records = []
            records.append(('L', hex(self._local)))
            records.append(('O', hex(self._other)))
            for d, v in self._state.iteritems():
                records.append(('F', '\0'.join([d] + v)))
            self._writerecords(records)
            self._dirty = False

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
        """Write current state on disk in a version 2 file"""
        f = self._repo.vfs(self.statepathv2, 'w')
        for key, data in records:
            assert len(key) == 1
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
        hash = util.sha1(fcl.path()).hexdigest()
        self._repo.vfs.write('merge/' + hash, fcl.data())
        self._state[fd] = ['u', hash, fcl.path(),
                           fca.path(), hex(fca.filenode()),
                           fco.path(), hex(fco.filenode()),
                           fcl.flags()]
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

    def unresolved(self):
        """Obtain the paths of unresolved files."""

        for f, entry in self._state.items():
            if entry[0] == 'u':
                yield f

    def resolve(self, dfile, wctx, labels=None):
        """rerun merge process for file path `dfile`"""
        if self[dfile] == 'r':
            return 0
        stateentry = self._state[dfile]
        state, hash, lfile, afile, anode, ofile, onode, flags = stateentry
        octx = self._repo[self._other]
        fcd = wctx[dfile]
        fco = octx[ofile]
        fca = self._repo.filectx(afile, fileid=anode)
        # "premerge" x flags
        flo = fco.flags()
        fla = fca.flags()
        if 'x' in flags + flo + fla and 'l' not in flags + flo + fla:
            if fca.node() == nullid:
                self._repo.ui.warn(_('warning: cannot merge flags for %s\n') %
                                   afile)
            elif flags == fla:
                flags = flo
        # restore local
        f = self._repo.vfs('merge/' + hash)
        self._repo.wwrite(dfile, f.read(), flags)
        f.close()
        r = filemerge.filemerge(self._repo, self._local, lfile, fcd, fco, fca,
                                labels=labels)
        if r is None:
            # no real conflict
            del self._state[dfile]
            self._dirty = True
        elif not r:
            self.mark(dfile, 'r')
        return r

def _checkunknownfile(repo, wctx, mctx, f, f2=None):
    if f2 is None:
        f2 = f
    return (os.path.isfile(repo.wjoin(f))
        and repo.wvfs.audit.check(f)
        and repo.dirstate.normalize(f) not in repo.dirstate
        and mctx[f2].cmp(wctx[f]))

def _checkunknownfiles(repo, wctx, mctx, force, actions):
    """
    Considers any actions that care about the presence of conflicting unknown
    files. For some actions, the result is to abort; for others, it is to
    choose a different action.
    """
    aborts = []
    if not force:
        for f, (m, args, msg) in actions.iteritems():
            if m in ('c', 'dc'):
                if _checkunknownfile(repo, wctx, mctx, f):
                    aborts.append(f)
            elif m == 'dg':
                if _checkunknownfile(repo, wctx, mctx, f, args[0]):
                    aborts.append(f)

    for f in sorted(aborts):
        repo.ui.warn(_("%s: untracked file differs\n") % f)
    if aborts:
        raise util.Abort(_("untracked files in working directory differ "
                           "from files in requested revision"))

    for f, (m, args, msg) in actions.iteritems():
        if m == 'c':
            actions[f] = ('g', args, msg)
        elif m == 'cm':
            fl2, anc = args
            different = _checkunknownfile(repo, wctx, mctx, f)
            if different:
                actions[f] = ('m', (f, f, None, False, anc),
                              "remote differs from untracked local")
            else:
                actions[f] = ('g', (fl2,), "remote created")

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
        for m in 'a', 'f', 'g', 'cd', 'dc':
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
            raise util.Abort(_("case-folding collision between %s and %s")
                             % (f, foldmap[fold]))
        foldmap[fold] = f

def manifestmerge(repo, wctx, p2, pa, branchmerge, force, partial,
                  acceptremote, followcopies):
    """
    Merge p1 and p2 with ancestor pa and generate merge action list

    branchmerge and force are as passed in to update
    partial = function to filter file lists
    acceptremote = accept the incoming changes without prompting
    """

    copy, movewithdir, diverge, renamedelete = {}, {}, {}, {}

    # manifests fetched in order are going to be faster, so prime the caches
    [x.manifest() for x in
     sorted(wctx.parents() + [p2, pa], key=lambda x: x.rev())]

    if followcopies:
        ret = copies.mergecopies(repo, wctx, p2, pa)
        copy, movewithdir, diverge, renamedelete = ret

    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(" branchmerge: %s, force: %s, partial: %s\n"
                  % (bool(branchmerge), bool(force), bool(partial)))
    repo.ui.debug(" ancestor: %s, local: %s, remote: %s\n" % (pa, wctx, p2))

    m1, m2, ma = wctx.manifest(), p2.manifest(), pa.manifest()
    copied = set(copy.values())
    copied.update(movewithdir.values())

    if '.hgsubstate' in m1:
        # check whether sub state is modified
        for s in sorted(wctx.substate):
            if wctx.sub(s).dirty():
                m1['.hgsubstate'] += '+'
                break

    # Compare manifests
    diff = m1.diff(m2)

    actions = {}
    for f, ((n1, fl1), (n2, fl2)) in diff.iteritems():
        if partial and not partial(f):
            continue
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
                        actions[f] = ('g', (fl2,), "remote is newer")
                elif nol and n2 == a: # remote only changed 'x'
                    actions[f] = ('e', (fl2,), "update permissions")
                elif nol and n1 == a: # local only changed 'x'
                    actions[f] = ('g', (fl1,), "remote is newer")
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
                        actions[f] = ('cd', None,  "prompt changed/deleted")
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
                    actions[f] = ('dc', (fl2,), "prompt deleted/changed")

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

def calculateupdates(repo, wctx, mctx, ancestors, branchmerge, force, partial,
                     acceptremote, followcopies):
    "Calculate the actions needed to merge mctx into wctx using ancestors"

    if len(ancestors) == 1: # default
        actions, diverge, renamedelete = manifestmerge(
            repo, wctx, mctx, ancestors[0], branchmerge, force, partial,
            acceptremote, followcopies)
        _checkunknownfiles(repo, wctx, mctx, force, actions)

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
                repo, wctx, mctx, ancestor, branchmerge, force, partial,
                acceptremote, followcopies)
            _checkunknownfiles(repo, wctx, mctx, force, actions)
            if diverge is None: # and renamedelete is None.
                # Arbitrarily pick warnings from first iteration
                diverge = diverge1
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
    i = 0
    for f, args, msg in actions:
        repo.ui.debug(" %s: %s -> g\n" % (f, msg))
        if verbose:
            repo.ui.note(_("getting %s\n") % f)
        wwrite(f, fctx(f).data(), args[0])
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

    updated, merged, removed, unresolved = 0, 0, 0, 0
    ms = mergestate(repo)
    ms.reset(wctx.p1().node(), mctx.node())
    moves = []
    for m, l in actions.items():
        l.sort()

    # prescan for merges
    for f, args, msg in actions['m']:
        f1, f2, fa, move, anc = args
        if f == '.hgsubstate': # merged internally
            continue
        repo.ui.debug(" preserving %s for resolve of %s\n" % (f1, f))
        fcl = wctx[f1]
        fco = mctx[f2]
        actx = repo[anc]
        if fa in actx:
            fca = actx[fa]
        else:
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

    def dirtysubstate():
        # mark '.hgsubstate' as possibly dirty forcibly, because
        # modified '.hgsubstate' is misunderstood as clean,
        # when both st_size/st_mtime of '.hgsubstate' aren't changed,
        # even if "submerge" fails and '.hgsubstate' is inconsistent
        repo.dirstate.normallookup('.hgsubstate')

    if [a for a in actions['r'] if a[0] == '.hgsubstate']:
        dirtysubstate()
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
        dirtysubstate()
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

    # keep (noop, just log it)
    for f, args, msg in actions['k']:
        repo.ui.debug(" %s: %s -> k\n" % (f, msg))
        # no progress

    # merge
    for f, args, msg in actions['m']:
        repo.ui.debug(" %s: %s -> m\n" % (f, msg))
        z += 1
        progress(_updating, z, item=f, total=numupdates, unit=_files)
        if f == '.hgsubstate': # subrepo states need updating
            dirtysubstate()
            subrepo.submerge(repo, wctx, mctx, wctx.ancestor(mctx),
                             overwrite)
            continue
        audit(f)
        r = ms.resolve(f, wctx, labels=labels)
        if r is not None and r > 0:
            unresolved += 1
        else:
            if r is None:
                updated += 1
            else:
                merged += 1

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

    ms.commit()
    progress(_updating, None, total=numupdates, unit=_files)

    return updated, merged, removed, unresolved

def recordupdates(repo, actions, branchmerge):
    "record merge actions to the dirstate"
    # remove (must come first)
    for f, args, msg in actions['r']:
        if branchmerge:
            repo.dirstate.remove(f)
        else:
            repo.dirstate.drop(f)

    # forget (must come first)
    for f, args, msg in actions['f']:
        repo.dirstate.drop(f)

    # re-add
    for f, args, msg in actions['a']:
        if not branchmerge:
            repo.dirstate.add(f)

    # exec change
    for f, args, msg in actions['e']:
        repo.dirstate.normallookup(f)

    # keep
    for f, args, msg in actions['k']:
        pass

    # get
    for f, args, msg in actions['g']:
        if branchmerge:
            repo.dirstate.otherparent(f)
        else:
            repo.dirstate.normal(f)

    # merge
    for f, args, msg in actions['m']:
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
    for f, args, msg in actions['dm']:
        f0, flag = args
        if branchmerge:
            repo.dirstate.add(f)
            repo.dirstate.remove(f0)
            repo.dirstate.copy(f0, f)
        else:
            repo.dirstate.normal(f)
            repo.dirstate.drop(f0)

    # directory rename, get
    for f, args, msg in actions['dg']:
        f0, flag = args
        if branchmerge:
            repo.dirstate.add(f)
            repo.dirstate.copy(f0, f)
        else:
            repo.dirstate.normal(f)

def update(repo, node, branchmerge, force, partial, ancestor=None,
           mergeancestor=False, labels=None):
    """
    Perform a merge between the working directory and the given node

    node = the node to update to, or None if unspecified
    branchmerge = whether to merge between branches
    force = whether to force branch merging or file overwriting
    partial = a function to filter file lists (dirstate not updated)
    mergeancestor = whether it is merging with an ancestor. If true,
      we should accept the incoming changes for any prompts that occur.
      If false, merging with an ancestor (fast-forward) is only allowed
      between different named branches. This flag is used by rebase extension
      as a temporary fix and should be avoided in general.

    The table below shows all the behaviors of the update command
    given the -c and -C or no options, whether the working directory
    is dirty, whether a revision is specified, and the relationship of
    the parent rev to the target rev (linear, on the same named
    branch, or on another named branch).

    This logic is tested by test-update-branches.t.

    -c  -C  dirty  rev  |  linear   same  cross
     n   n    n     n   |    ok     (1)     x
     n   n    n     y   |    ok     ok     ok
     n   n    y     n   |   merge   (2)    (2)
     n   n    y     y   |   merge   (3)    (3)
     n   y    *     *   |    ---  discard  ---
     y   n    y     *   |    ---    (4)    ---
     y   n    n     *   |    ---    ok     ---
     y   y    *     *   |    ---    (5)    ---

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
    wlock = repo.wlock()
    try:
        wc = repo[None]
        pl = wc.parents()
        p1 = pl[0]
        pas = [None]
        if ancestor is not None:
            pas = [repo[ancestor]]

        if node is None:
            # Here is where we should consider bookmarks, divergent bookmarks,
            # foreground changesets (successors), and tip of current branch;
            # but currently we are only checking the branch tips.
            try:
                node = repo.branchtip(wc.branch())
            except errormod.RepoLookupError:
                if wc.branch() == 'default': # no default branch!
                    node = repo.lookup('tip') # update to tip
                else:
                    raise util.Abort(_("branch %s not found") % wc.branch())

            if p1.obsolete() and not p1.children():
                # allow updating to successors
                successors = obsolete.successorssets(repo, p1.node())

                # behavior of certain cases is as follows,
                #
                # divergent changesets: update to highest rev, similar to what
                #     is currently done when there are more than one head
                #     (i.e. 'tip')
                #
                # replaced changesets: same as divergent except we know there
                # is no conflict
                #
                # pruned changeset: no update is done; though, we could
                #     consider updating to the first non-obsolete parent,
                #     similar to what is current done for 'hg prune'

                if successors:
                    # flatten the list here handles both divergent (len > 1)
                    # and the usual case (len = 1)
                    successors = [n for sub in successors for n in sub]

                    # get the max revision for the given successors set,
                    # i.e. the 'tip' of a set
                    node = repo.revs('max(%ln)', successors).first()
                    pas = [p1]

        overwrite = force and not branchmerge

        p2 = repo[node]
        if pas[0] is None:
            if repo.ui.config('merge', 'preferancestor', '*') == '*':
                cahs = repo.changelog.commonancestorsheads(p1.node(), p2.node())
                pas = [repo[anc] for anc in (sorted(cahs) or [nullid])]
            else:
                pas = [p1.ancestor(p2, warn=branchmerge)]

        fp1, fp2, xp1, xp2 = p1.node(), p2.node(), str(p1), str(p2)

        ### check phase
        if not overwrite and len(pl) > 1:
            raise util.Abort(_("outstanding uncommitted merge"))
        if branchmerge:
            if pas == [p2]:
                raise util.Abort(_("merging with a working directory ancestor"
                                   " has no effect"))
            elif pas == [p1]:
                if not mergeancestor and p1.branch() == p2.branch():
                    raise util.Abort(_("nothing to merge"),
                                     hint=_("use 'hg update' "
                                            "or check 'hg heads'"))
            if not force and (wc.files() or wc.deleted()):
                raise util.Abort(_("uncommitted changes"),
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
                        raise util.Abort(msg, hint=hint)
                    else:  # node is none
                        msg = _("not a linear update")
                        hint = _("merge or update --check to force update")
                        raise util.Abort(msg, hint=hint)
                else:
                    # Allow jumping branches if clean and specific rev given
                    pas = [p1]

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
            repo, wc, p2, pas, branchmerge, force, partial, mergeancestor,
            followcopies)
        # Convert to dictionary-of-lists format
        actions = dict((m, []) for m in 'a f g cd dc r dm dg m e k'.split())
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

        # Prompt and create actions. TODO: Move this towards resolve phase.
        for f, args, msg in sorted(actions['cd']):
            if repo.ui.promptchoice(
                _("local changed %s which remote deleted\n"
                  "use (c)hanged version or (d)elete?"
                  "$$ &Changed $$ &Delete") % f, 0):
                actions['r'].append((f, None, "prompt delete"))
            else:
                actions['a'].append((f, None, "prompt keep"))
        del actions['cd'][:]

        for f, args, msg in sorted(actions['dc']):
            flags, = args
            if repo.ui.promptchoice(
                _("remote changed %s which local deleted\n"
                  "use (c)hanged version or leave (d)eleted?"
                  "$$ &Changed $$ &Deleted") % f, 0) == 0:
                actions['g'].append((f, (flags,), "prompt recreating"))
        del actions['dc'][:]

        ### apply phase
        if not branchmerge: # just jump to the new rev
            fp1, fp2, xp1, xp2 = fp2, nullid, xp2, ''
        if not partial:
            repo.hook('preupdate', throw=True, parent1=xp1, parent2=xp2)
            # note that we're in the middle of an update
            repo.vfs.write('updatestate', p2.hex())

        stats = applyupdates(repo, actions, wc, p2, overwrite, labels=labels)

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

        if not partial:
            repo.dirstate.beginparentchange()
            repo.setparents(fp1, fp2)
            recordupdates(repo, actions, branchmerge)
            # update completed, clear state
            util.unlink(repo.join('updatestate'))

            if not branchmerge:
                repo.dirstate.setbranch(p2.branch())
            repo.dirstate.endparentchange()
    finally:
        wlock.release()

    if not partial:
        def updatehook(parent1=xp1, parent2=xp2, error=stats[3]):
            repo.hook('update', parent1=parent1, parent2=parent2, error=error)
        repo._afterlock(updatehook)
    return stats

def graft(repo, ctx, pctx, labels):
    """Do a graft-like merge.

    This is a merge where the merge ancestor is chosen such that one
    or more changesets are grafted onto the current changeset. In
    addition to the merge, this fixes up the dirstate to include only
    a single parent and tries to duplicate any renames/copies
    appropriately.

    ctx - changeset to rebase
    pctx - merge base, usually ctx.p1()
    labels - merge labels eg ['local', 'graft']

    """
    # If we're grafting a descendant onto an ancestor, be sure to pass
    # mergeancestor=True to update. This does two things: 1) allows the merge if
    # the destination is the same as the parent of the ctx (so we can use graft
    # to copy commits), and 2) informs update that the incoming changes are
    # newer than the destination so it doesn't prompt about "remote changed foo
    # which local deleted".
    mergeancestor = repo.changelog.isancestor(repo['.'].node(), ctx.node())

    stats = update(repo, ctx.node(), True, True, False, pctx.node(),
                   mergeancestor=mergeancestor, labels=labels)

    # drop the second merge parent
    repo.dirstate.beginparentchange()
    repo.setparents(repo['.'].node(), nullid)
    repo.dirstate.write()
    # fix up dirstate for copies and renames
    copies.duplicatecopies(repo, ctx.rev(), pctx.rev())
    repo.dirstate.endparentchange()
    return stats

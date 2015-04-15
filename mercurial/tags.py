# tags.py - read tag info from local repository
#
# Copyright 2009 Matt Mackall <mpm@selenic.com>
# Copyright 2009 Greg Ward <greg@gerg.ca>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Currently this module only deals with reading and caching tags.
# Eventually, it could take care of updating (adding/removing/moving)
# tags too.

from node import nullid, bin, hex, short
from i18n import _
import util
import encoding
import error
from array import array
import errno
import time

# Tags computation can be expensive and caches exist to make it fast in
# the common case.
#
# The "hgtagsfnodes1" cache file caches the .hgtags filenode values for
# each revision in the repository. The file is effectively an array of
# fixed length records. Read the docs for "hgtagsfnodescache" for technical
# details.
#
# The .hgtags filenode cache grows in proportion to the length of the
# changelog. The file is truncated when the # changelog is stripped.
#
# The purpose of the filenode cache is to avoid the most expensive part
# of finding global tags, which is looking up the .hgtags filenode in the
# manifest for each head. This can take dozens or over 100ms for
# repositories with very large manifests. Multiplied by dozens or even
# hundreds of heads and there is a significant performance concern.
#
# The "tags" cache stores information about heads and the history of tags.
#
# The cache file consists of two parts. The first part maps head nodes
# to .hgtags filenodes. The second part is a history of tags. The two
# parts are separated by an empty line.
#
# The filenodes part of "tags" has effectively been superseded by
# "hgtagsfnodes1." It is being kept around for backwards compatbility.
#
# The first part consists of lines of the form:
#
#   <headrev> <headnode> [<hgtagsnode>]
#
# <headrev> is an integer revision and <headnode> is a 40 character hex
# node for that changeset. These redundantly identify a repository
# head from the time the cache was written.
#
# <tagnode> is the filenode of .hgtags on that head. Heads with no .hgtags
# file will have no <hgtagsnode> (just 2 values per line).
#
# The filenode cache is ordered from tip to oldest (which is part of why
# <headrev> is there: a quick check of the tip from when the cache was
# written against the current tip is all that is needed to check whether
# the cache is up to date).
#
# The second part of the tags cache consists of lines of the form:
#
#   <node> <tag>
#
# (This format is identical to that of .hgtags files.)
#
# <tag> is the tag name and <node> is the 40 character hex changeset
# the tag is associated with.
#
# Tags are written sorted by tag name.
#
# Tags associated with multiple changesets have an entry for each changeset.
# The most recent changeset (in terms of revlog ordering for the head
# setting it) for each tag is last.

def findglobaltags(ui, repo, alltags, tagtypes):
    '''Find global tags in a repo.

    "alltags" maps tag name to (node, hist) 2-tuples.

    "tagtypes" maps tag name to tag type. Global tags always have the
    "global" tag type.

    The "alltags" and "tagtypes" dicts are updated in place. Empty dicts
    should be passed in.

    The tags cache is read and updated as a side-effect of calling.
    '''
    # This is so we can be lazy and assume alltags contains only global
    # tags when we pass it to _writetagcache().
    assert len(alltags) == len(tagtypes) == 0, \
           "findglobaltags() should be called first"

    (heads, tagfnode, cachetags, shouldwrite) = _readtagcache(ui, repo)
    if cachetags is not None:
        assert not shouldwrite
        # XXX is this really 100% correct?  are there oddball special
        # cases where a global tag should outrank a local tag but won't,
        # because cachetags does not contain rank info?
        _updatetags(cachetags, 'global', alltags, tagtypes)
        return

    seen = set()  # set of fnode
    fctx = None
    for head in reversed(heads):  # oldest to newest
        assert head in repo.changelog.nodemap, \
               "tag cache returned bogus head %s" % short(head)

        fnode = tagfnode.get(head)
        if fnode and fnode not in seen:
            seen.add(fnode)
            if not fctx:
                fctx = repo.filectx('.hgtags', fileid=fnode)
            else:
                fctx = fctx.filectx(fnode)

            filetags = _readtags(ui, repo, fctx.data().splitlines(), fctx)
            _updatetags(filetags, 'global', alltags, tagtypes)

    # and update the cache (if necessary)
    if shouldwrite:
        _writetagcache(ui, repo, heads, tagfnode, alltags)

def readlocaltags(ui, repo, alltags, tagtypes):
    '''Read local tags in repo. Update alltags and tagtypes.'''
    try:
        data = repo.vfs.read("localtags")
    except IOError, inst:
        if inst.errno != errno.ENOENT:
            raise
        return

    # localtags is in the local encoding; re-encode to UTF-8 on
    # input for consistency with the rest of this module.
    filetags = _readtags(
        ui, repo, data.splitlines(), "localtags",
        recode=encoding.fromlocal)

    # remove tags pointing to invalid nodes
    cl = repo.changelog
    for t in filetags.keys():
        try:
            cl.rev(filetags[t][0])
        except (LookupError, ValueError):
            del filetags[t]

    _updatetags(filetags, "local", alltags, tagtypes)

def _readtaghist(ui, repo, lines, fn, recode=None, calcnodelines=False):
    '''Read tag definitions from a file (or any source of lines).

    This function returns two sortdicts with similar information:

    - the first dict, bintaghist, contains the tag information as expected by
      the _readtags function, i.e. a mapping from tag name to (node, hist):
        - node is the node id from the last line read for that name,
        - hist is the list of node ids previously associated with it (in file
          order). All node ids are binary, not hex.

    - the second dict, hextaglines, is a mapping from tag name to a list of
      [hexnode, line number] pairs, ordered from the oldest to the newest node.

    When calcnodelines is False the hextaglines dict is not calculated (an
    empty dict is returned). This is done to improve this function's
    performance in cases where the line numbers are not needed.
    '''

    bintaghist = util.sortdict()
    hextaglines = util.sortdict()
    count = 0

    def warn(msg):
        ui.warn(_("%s, line %s: %s\n") % (fn, count, msg))

    for nline, line in enumerate(lines):
        count += 1
        if not line:
            continue
        try:
            (nodehex, name) = line.split(" ", 1)
        except ValueError:
            warn(_("cannot parse entry"))
            continue
        name = name.strip()
        if recode:
            name = recode(name)
        try:
            nodebin = bin(nodehex)
        except TypeError:
            warn(_("node '%s' is not well formed") % nodehex)
            continue

        # update filetags
        if calcnodelines:
            # map tag name to a list of line numbers
            if name not in hextaglines:
                hextaglines[name] = []
            hextaglines[name].append([nodehex, nline])
            continue
        # map tag name to (node, hist)
        if name not in bintaghist:
            bintaghist[name] = []
        bintaghist[name].append(nodebin)
    return bintaghist, hextaglines

def _readtags(ui, repo, lines, fn, recode=None, calcnodelines=False):
    '''Read tag definitions from a file (or any source of lines).

    Returns a mapping from tag name to (node, hist).

    "node" is the node id from the last line read for that name. "hist"
    is the list of node ids previously associated with it (in file order).
    All node ids are binary, not hex.
    '''
    filetags, nodelines = _readtaghist(ui, repo, lines, fn, recode=recode,
                                       calcnodelines=calcnodelines)
    for tag, taghist in filetags.items():
        filetags[tag] = (taghist[-1], taghist[:-1])
    return filetags

def _updatetags(filetags, tagtype, alltags, tagtypes):
    '''Incorporate the tag info read from one file into the two
    dictionaries, alltags and tagtypes, that contain all tag
    info (global across all heads plus local).'''

    for name, nodehist in filetags.iteritems():
        if name not in alltags:
            alltags[name] = nodehist
            tagtypes[name] = tagtype
            continue

        # we prefer alltags[name] if:
        #  it supersedes us OR
        #  mutual supersedes and it has a higher rank
        # otherwise we win because we're tip-most
        anode, ahist = nodehist
        bnode, bhist = alltags[name]
        if (bnode != anode and anode in bhist and
            (bnode not in ahist or len(bhist) > len(ahist))):
            anode = bnode
        else:
            tagtypes[name] = tagtype
        ahist.extend([n for n in bhist if n not in ahist])
        alltags[name] = anode, ahist

def _readtagcache(ui, repo):
    '''Read the tag cache.

    Returns a tuple (heads, fnodes, cachetags, shouldwrite).

    If the cache is completely up-to-date, "cachetags" is a dict of the
    form returned by _readtags() and "heads" and "fnodes" are None and
    "shouldwrite" is False.

    If the cache is not up to date, "cachetags" is None. "heads" is a list
    of all heads currently in the repository, ordered from tip to oldest.
    "fnodes" is a mapping from head to .hgtags filenode. "shouldwrite" is
    True.

    If the cache is not up to date, the caller is responsible for reading tag
    info from each returned head. (See findglobaltags().)
    '''

    try:
        cachefile = repo.vfs('cache/tags', 'r')
        # force reading the file for static-http
        cachelines = iter(cachefile)
    except IOError:
        cachefile = None

    cacherevs = []  # list of headrev
    cacheheads = [] # list of headnode
    cachefnode = {} # map headnode to filenode
    if cachefile:
        try:
            for line in cachelines:
                if line == "\n":
                    break
                line = line.split()
                cacherevs.append(int(line[0]))
                headnode = bin(line[1])
                cacheheads.append(headnode)
                if len(line) == 3:
                    fnode = bin(line[2])
                    cachefnode[headnode] = fnode
        except Exception:
            # corruption of the tags cache, just recompute it
            cacheheads = []
            cacherevs = []
            cachefnode = {}

    tipnode = repo.changelog.tip()
    tiprev = len(repo.changelog) - 1

    # Case 1 (common): tip is the same, so nothing has changed.
    # (Unchanged tip trivially means no changesets have been added.
    # But, thanks to localrepository.destroyed(), it also means none
    # have been destroyed by strip or rollback.)
    if cacheheads and cacheheads[0] == tipnode and cacherevs[0] == tiprev:
        tags = _readtags(ui, repo, cachelines, cachefile.name)
        cachefile.close()
        return (None, None, tags, False)
    if cachefile:
        cachefile.close()               # ignore rest of file

    repoheads = repo.heads()
    # Case 2 (uncommon): empty repo; get out quickly and don't bother
    # writing an empty cache.
    if repoheads == [nullid]:
        return ([], {}, {}, False)

    # Case 3 (uncommon): cache file missing or empty.

    # Case 4 (uncommon): tip rev decreased.  This should only happen
    # when we're called from localrepository.destroyed().  Refresh the
    # cache so future invocations will not see disappeared heads in the
    # cache.

    # Case 5 (common): tip has changed, so we've added/replaced heads.

    # As it happens, the code to handle cases 3, 4, 5 is the same.

    # N.B. in case 4 (nodes destroyed), "new head" really means "newly
    # exposed".
    if not len(repo.file('.hgtags')):
        # No tags have ever been committed, so we can avoid a
        # potentially expensive search.
        return (repoheads, cachefnode, None, True)

    starttime = time.time()

    newheads = [head
                for head in repoheads
                if head not in set(cacheheads)]

    # Now we have to lookup the .hgtags filenode for every new head.
    # This is the most expensive part of finding tags, so performance
    # depends primarily on the size of newheads.  Worst case: no cache
    # file, so newheads == repoheads.
    fnodescache = hgtagsfnodescache(repo.unfiltered())
    for head in reversed(newheads):
        fnode = fnodescache.getfnode(head)
        if fnode != nullid:
            cachefnode[head] = fnode

    fnodescache.write()

    duration = time.time() - starttime
    ui.log('tagscache',
           '%d/%d cache hits/lookups in %0.4f '
           'seconds\n',
           fnodescache.hitcount, fnodescache.lookupcount, duration)

    # Caller has to iterate over all heads, but can use the filenodes in
    # cachefnode to get to each .hgtags revision quickly.
    return (repoheads, cachefnode, None, True)

def _writetagcache(ui, repo, heads, tagfnode, cachetags):
    try:
        cachefile = repo.vfs('cache/tags', 'w', atomictemp=True)
    except (OSError, IOError):
        return

    ui.log('tagscache', 'writing tags cache file with %d heads and %d tags\n',
            len(heads), len(cachetags))

    realheads = repo.heads()            # for sanity checks below
    for head in heads:
        # temporary sanity checks; these can probably be removed
        # once this code has been in crew for a few weeks
        assert head in repo.changelog.nodemap, \
               'trying to write non-existent node %s to tag cache' % short(head)
        assert head in realheads, \
               'trying to write non-head %s to tag cache' % short(head)
        assert head != nullid, \
               'trying to write nullid to tag cache'

        # This can't fail because of the first assert above.  When/if we
        # remove that assert, we might want to catch LookupError here
        # and downgrade it to a warning.
        rev = repo.changelog.rev(head)

        fnode = tagfnode.get(head)
        if fnode:
            cachefile.write('%d %s %s\n' % (rev, hex(head), hex(fnode)))
        else:
            cachefile.write('%d %s\n' % (rev, hex(head)))

    # Tag names in the cache are in UTF-8 -- which is the whole reason
    # we keep them in UTF-8 throughout this module.  If we converted
    # them local encoding on input, we would lose info writing them to
    # the cache.
    cachefile.write('\n')
    for (name, (node, hist)) in sorted(cachetags.iteritems()):
        for n in hist:
            cachefile.write("%s %s\n" % (hex(n), name))
        cachefile.write("%s %s\n" % (hex(node), name))

    try:
        cachefile.close()
    except (OSError, IOError):
        pass

_fnodescachefile = 'cache/hgtagsfnodes1'
_fnodesrecsize = 4 + 20 # changeset fragment + filenode
_fnodesmissingrec = '\xff' * 24

class hgtagsfnodescache(object):
    """Persistent cache mapping revisions to .hgtags filenodes.

    The cache is an array of records. Each item in the array corresponds to
    a changelog revision. Values in the array contain the first 4 bytes of
    the node hash and the 20 bytes .hgtags filenode for that revision.

    The first 4 bytes are present as a form of verification. Repository
    stripping and rewriting may change the node at a numeric revision in the
    changelog. The changeset fragment serves as a verifier to detect
    rewriting. This logic is shared with the rev branch cache (see
    branchmap.py).

    The instance holds in memory the full cache content but entries are
    only parsed on read.

    Instances behave like lists. ``c[i]`` works where i is a rev or
    changeset node. Missing indexes are populated automatically on access.
    """
    def __init__(self, repo):
        assert repo.filtername is None

        self._repo = repo

        # Only for reporting purposes.
        self.lookupcount = 0
        self.hitcount = 0

        self._raw = array('c')

        data = repo.vfs.tryread(_fnodescachefile)
        self._raw.fromstring(data)

        # The end state of self._raw is an array that is of the exact length
        # required to hold a record for every revision in the repository.
        # We truncate or extend the array as necessary. self._dirtyoffset is
        # defined to be the start offset at which we need to write the output
        # file. This offset is also adjusted when new entries are calculated
        # for array members.
        cllen = len(repo.changelog)
        wantedlen = cllen * _fnodesrecsize
        rawlen = len(self._raw)

        self._dirtyoffset = None

        if rawlen < wantedlen:
            self._dirtyoffset = rawlen
            self._raw.extend('\xff' * (wantedlen - rawlen))
        elif rawlen > wantedlen:
            # There's no easy way to truncate array instances. This seems
            # slightly less evil than copying a potentially large array slice.
            for i in range(rawlen - wantedlen):
                self._raw.pop()
            self._dirtyoffset = len(self._raw)

    def getfnode(self, node):
        """Obtain the filenode of the .hgtags file at a specified revision.

        If the value is in the cache, the entry will be validated and returned.
        Otherwise, the filenode will be computed and returned.

        If an .hgtags does not exist at the specified revision, nullid is
        returned.
        """
        ctx = self._repo[node]
        rev = ctx.rev()

        self.lookupcount += 1

        offset = rev * _fnodesrecsize
        record = self._raw[offset:offset + _fnodesrecsize].tostring()
        properprefix = node[0:4]

        # Validate and return existing entry.
        if record != _fnodesmissingrec:
            fileprefix = record[0:4]

            if fileprefix == properprefix:
                self.hitcount += 1
                return record[4:]

            # Fall through.

        # If we get here, the entry is either missing or invalid. Populate it.
        try:
            fnode = ctx.filenode('.hgtags')
        except error.LookupError:
            # No .hgtags file on this revision.
            fnode = nullid

        # Slices on array instances only accept other array.
        entry = array('c', properprefix + fnode)
        self._raw[offset:offset + _fnodesrecsize] = entry
        # self._dirtyoffset could be None.
        self._dirtyoffset = min(self._dirtyoffset, offset) or 0

        return fnode

    def write(self):
        """Perform all necessary writes to cache file.

        This may no-op if no writes are needed or if a write lock could
        not be obtained.
        """
        if self._dirtyoffset is None:
            return

        data = self._raw[self._dirtyoffset:]
        if not data:
            return

        repo = self._repo

        try:
            lock = repo.wlock(wait=False)
        except error.LockHeld:
            repo.ui.log('tagscache',
                        'not writing .hg/%s because lock held\n' %
                        (_fnodescachefile))
            return

        try:
            try:
                f = repo.vfs.open(_fnodescachefile, 'ab')
                try:
                    # if the file has been truncated
                    actualoffset = f.tell()
                    if actualoffset < self._dirtyoffset:
                        self._dirtyoffset = actualoffset
                        data = self._raw[self._dirtyoffset:]
                    f.seek(self._dirtyoffset)
                    f.truncate()
                    repo.ui.log('tagscache',
                                'writing %d bytes to %s\n' % (
                                len(data), _fnodescachefile))
                    f.write(data)
                    self._dirtyoffset = None
                finally:
                    f.close()
            except (IOError, OSError), inst:
                repo.ui.log('tagscache',
                            "couldn't write %s: %s\n" % (
                            _fnodescachefile, inst))
        finally:
            lock.release()

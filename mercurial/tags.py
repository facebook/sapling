# tags.py - read tag info from local repository
#
# Copyright 2009 Matt Mackall <mpm@selenic.com>
# Copyright 2009 Greg Ward <greg@gerg.ca>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

# Currently this module only deals with reading tags.  Soon it will grow
# support for caching tag info.  Eventually, it could take care of
# updating (adding/removing/moving) tags too.

from node import bin, hex
from i18n import _
import encoding
import error

def findglobaltags(ui, repo, alltags, tagtypes):
    '''Find global tags in repo by reading .hgtags from every head that
    has a distinct version of it.  Updates the dicts alltags, tagtypes
    in place: alltags maps tag name to (node, hist) pair (see _readtags()
    below), and tagtypes maps tag name to tag type ('global' in this
    case).'''

    seen = set()
    fctx = None
    ctxs = []                       # list of filectx
    for node in repo.heads():
        try:
            fnode = repo[node].filenode('.hgtags')
        except error.LookupError:
            continue
        if fnode not in seen:
            seen.add(fnode)
            if not fctx:
                fctx = repo.filectx('.hgtags', fileid=fnode)
            else:
                fctx = fctx.filectx(fnode)
            ctxs.append(fctx)

    # read the tags file from each head, ending with the tip
    for fctx in reversed(ctxs):
        filetags = _readtags(
            ui, repo, fctx.data().splitlines(), fctx)
        _updatetags(filetags, "global", alltags, tagtypes)

def readlocaltags(ui, repo, alltags, tagtypes):
    '''Read local tags in repo.  Update alltags and tagtypes.'''
    try:
        data = encoding.fromlocal(repo.opener("localtags").read())
        # localtags are stored in the local character set
        # while the internal tag table is stored in UTF-8
        filetags = _readtags(
            ui, repo, data.splitlines(), "localtags")
        _updatetags(filetags, "local", alltags, tagtypes)
    except IOError:
        pass

def _readtags(ui, repo, lines, fn):
    '''Read tag definitions from a file (or any source of lines).
    Return a mapping from tag name to (node, hist): node is the node id
    from the last line read for that name, and hist is the list of node
    ids previously associated with it (in file order).  All node ids are
    binary, not hex.'''

    filetags = {}               # map tag name to (node, hist)
    count = 0

    def warn(msg):
        ui.warn(_("%s, line %s: %s\n") % (fn, count, msg))

    for line in lines:
        count += 1
        if not line:
            continue
        try:
            (nodehex, name) = line.split(" ", 1)
        except ValueError:
            warn(_("cannot parse entry"))
            continue
        name = encoding.tolocal(name.strip()) # stored in UTF-8
        try:
            nodebin = bin(nodehex)
        except TypeError:
            warn(_("node '%s' is not well formed") % nodehex)
            continue
        if nodebin not in repo.changelog.nodemap:
            # silently ignore as pull -r might cause this
            continue

        # update filetags
        hist = []
        if name in filetags:
            n, hist = filetags[name]
            hist.append(n)
        filetags[name] = (nodebin, hist)
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
        #  it supercedes us OR
        #  mutual supercedes and it has a higher rank
        # otherwise we win because we're tip-most
        anode, ahist = nodehist
        bnode, bhist = alltags[name]
        if (bnode != anode and anode in bhist and
            (bnode not in ahist or len(bhist) > len(ahist))):
            anode = bnode
        ahist.extend([n for n in bhist if n not in ahist])
        alltags[name] = anode, ahist
        tagtypes[name] = tagtype


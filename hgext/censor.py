# Copyright (C) 2015 - Mike Edgar <adgar@google.com>
#
# This extension enables removal of file content at a given revision,
# rewriting the data/metadata of successive revisions to preserve revision log
# integrity.

"""erase file content at a given revision

The censor command instructs Mercurial to erase all content of a file at a given
revision *without updating the changeset hash.* This allows existing history to
remain valid while preventing future clones/pulls from receiving the erased
data.

Typical uses for censor are due to security or legal requirements, including::

 * Passwords, private keys, crytographic material
 * Licensed data/code/libraries for which the license has expired
 * Personally Identifiable Information or other private data

Censored nodes can interrupt mercurial's typical operation whenever the excised
data needs to be materialized. Some commands, like ``hg cat``/``hg revert``,
simply fail when asked to produce censored data. Others, like ``hg verify`` and
``hg update``, must be capable of tolerating censored data to continue to
function in a meaningful way. Such commands only tolerate censored file
revisions if they are allowed by the "censor.policy=ignore" config option.
"""

from mercurial.node import short
from mercurial import cmdutil, error, filelog, revlog, scmutil, util
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

@command('censor',
    [('r', 'rev', '', _('censor file from specified revision'), _('REV')),
     ('t', 'tombstone', '', _('replacement tombstone data'), _('TEXT'))],
    _('-r REV [-t TEXT] [FILE]'))
def censor(ui, repo, path, rev='', tombstone='', **opts):
    if not path:
        raise util.Abort(_('must specify file path to censor'))
    if not rev:
        raise util.Abort(_('must specify revision to censor'))

    flog = repo.file(path)
    if not len(flog):
        raise util.Abort(_('cannot censor file with no history'))

    rev = scmutil.revsingle(repo, rev, rev).rev()
    try:
        ctx = repo[rev]
    except KeyError:
        raise util.Abort(_('invalid revision identifier %s') % rev)

    try:
        fctx = ctx.filectx(path)
    except error.LookupError:
        raise util.Abort(_('file does not exist at revision %s') % rev)

    fnode = fctx.filenode()
    headctxs = [repo[c] for c in repo.heads()]
    heads = [c for c in headctxs if path in c and c.filenode(path) == fnode]
    if heads:
        headlist = ', '.join([short(c.node()) for c in heads])
        raise util.Abort(_('cannot censor file in heads (%s)') % headlist,
            hint=_('clean/delete and commit first'))

    wctx = repo[None]
    wp = wctx.parents()
    if ctx.node() in [p.node() for p in wp]:
        raise util.Abort(_('cannot censor working directory'),
            hint=_('clean/delete/update first'))

    flogv = flog.version & 0xFFFF
    if flogv != revlog.REVLOGNG:
        raise util.Abort(
            _('censor does not support revlog version %d') % (flogv,))

    tombstone = filelog.packmeta({"censored": tombstone}, "")

    crev = fctx.filerev()

    if len(tombstone) > flog.rawsize(crev):
        raise util.Abort(_(
            'censor tombstone must be no longer than censored data'))

    # Using two files instead of one makes it easy to rewrite entry-by-entry
    idxread = repo.svfs(flog.indexfile, 'r')
    idxwrite = repo.svfs(flog.indexfile, 'wb', atomictemp=True)
    if flog.version & revlog.REVLOGNGINLINEDATA:
        dataread, datawrite = idxread, idxwrite
    else:
        dataread = repo.svfs(flog.datafile, 'r')
        datawrite = repo.svfs(flog.datafile, 'wb', atomictemp=True)

    # Copy all revlog data up to the entry to be censored.
    rio = revlog.revlogio()
    offset = flog.start(crev)

    for chunk in util.filechunkiter(idxread, limit=crev * rio.size):
        idxwrite.write(chunk)
    for chunk in util.filechunkiter(dataread, limit=offset):
        datawrite.write(chunk)

    def rewriteindex(r, newoffs, newdata=None):
        """Rewrite the index entry with a new data offset and optional new data.

        The newdata argument, if given, is a tuple of three positive integers:
        (new compressed, new uncompressed, added flag bits).
        """
        offlags, comp, uncomp, base, link, p1, p2, nodeid = flog.index[r]
        flags = revlog.gettype(offlags)
        if newdata:
            comp, uncomp, nflags = newdata
            flags |= nflags
        offlags = revlog.offset_type(newoffs, flags)
        e = (offlags, comp, uncomp, r, link, p1, p2, nodeid)
        idxwrite.write(rio.packentry(e, None, flog.version, r))
        idxread.seek(rio.size, 1)

    def rewrite(r, offs, data, nflags=revlog.REVIDX_DEFAULT_FLAGS):
        """Write the given full text to the filelog with the given data offset.

        Returns:
            The integer number of data bytes written, for tracking data offsets.
        """
        flag, compdata = flog.compress(data)
        newcomp = len(flag) + len(compdata)
        rewriteindex(r, offs, (newcomp, len(data), nflags))
        datawrite.write(flag)
        datawrite.write(compdata)
        dataread.seek(flog.length(r), 1)
        return newcomp

    # Rewrite censored revlog entry with (padded) tombstone data.
    pad = ' ' * (flog.rawsize(crev) - len(tombstone))
    offset += rewrite(crev, offset, tombstone + pad, revlog.REVIDX_ISCENSORED)

    # Rewrite all following filelog revisions fixing up offsets and deltas.
    for srev in xrange(crev + 1, len(flog)):
        if crev in flog.parentrevs(srev):
            # Immediate children of censored node must be re-added as fulltext.
            try:
                revdata = flog.revision(srev)
            except error.CensoredNodeError as e:
                revdata = e.tombstone
            dlen = rewrite(srev, offset, revdata)
        else:
            # Copy any other revision data verbatim after fixing up the offset.
            rewriteindex(srev, offset)
            dlen = flog.length(srev)
            for chunk in util.filechunkiter(dataread, limit=dlen):
                datawrite.write(chunk)
        offset += dlen

    idxread.close()
    idxwrite.close()
    if dataread is not idxread:
        dataread.close()
        datawrite.close()

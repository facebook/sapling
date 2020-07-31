# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# changelog.py - changelog class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from typing import IO, Any, Dict, List, Optional, Union

import bindings

from . import encoding, error, mdiff, revlog, util, visibility
from .i18n import _
from .node import bbin, bin, hex, nullid, nullrev, wdirid, wdirrev
from .pycompat import decodeutf8, encodeutf8, iteritems, range
from .thirdparty import attr


_defaultextra = {"branch": "default"}

textwithheader = revlog.textwithheader


def _string_escape(text):
    """
    >>> from .pycompat import bytechr as chr
    >>> d = {b'nl': chr(10), b'bs': chr(92), b'cr': chr(13), b'nul': chr(0)}
    >>> s = b"ab%(nl)scd%(bs)s%(bs)sn%(nul)sab%(cr)scd%(bs)s%(nl)s" % d
    >>> s
    'ab\\ncd\\\\\\\\n\\x00ab\\rcd\\\\\\n'
    >>> res = _string_escape(s)
    >>> s == util.unescapestr(res)
    True
    """
    # subset of the string_escape codec
    text = text.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r")
    return text.replace("\0", "\\0")


def decodeextra(text):
    # type: (bytes) -> Dict[str, str]
    """
    >>> from .pycompat import bytechr as chr
    >>> sorted(decodeextra(encodeextra({b'foo': b'bar', b'baz': chr(0) + b'2'})
    ...                    ).items())
    [('baz', '\\x002'), ('branch', 'default'), ('foo', 'bar')]
    >>> sorted(decodeextra(encodeextra({b'foo': b'bar',
    ...                                 b'baz': chr(92) + chr(0) + b'2'})
    ...                    ).items())
    [('baz', '\\\\\\x002'), ('branch', 'default'), ('foo', 'bar')]
    """
    extra = _defaultextra.copy()
    for l in text.split(b"\0"):
        if l:
            if b"\\0" in l:
                # fix up \0 without getting into trouble with \\0
                l = l.replace(b"\\\\", b"\\\\\n")
                l = l.replace(b"\\0", b"\0")
                l = l.replace(b"\n", b"")
            k, v = util.unescapestr(l).split(":", 1)
            extra[k] = v
    return extra


def encodeextra(d):
    for k, v in iteritems(d):
        if not isinstance(v, str):
            raise ValueError("extra '%s' should be type str not %s" % (k, v.__class__))

    # keys must be sorted to produce a deterministic changelog entry
    items = [_string_escape("%s:%s" % (k, d[k])) for k in sorted(d)]
    return "\0".join(items)


def stripdesc(desc):
    """strip trailing whitespace and leading and trailing empty lines"""
    return "\n".join([l.rstrip() for l in desc.splitlines()]).strip("\n")


class appender(object):
    """the changelog index must be updated last on disk, so we use this class
    to delay writes to it"""

    def __init__(self, vfs, name, mode, buf):
        self.data = buf
        fp = vfs(name, mode)
        self.fp = fp
        self.offset = fp.tell()
        self.size = vfs.fstat(fp).st_size
        self._end = self.size

    def end(self):
        return self._end

    def tell(self):
        return self.offset

    def flush(self):
        pass

    def close(self):
        self.fp.close()

    def seek(self, offset, whence=0):
        """virtual file offset spans real file and data"""
        if whence == 0:
            self.offset = offset
        elif whence == 1:
            self.offset += offset
        elif whence == 2:
            self.offset = self.end() + offset
        if self.offset < self.size:
            self.fp.seek(self.offset)

    def read(self, count=-1):
        """only trick here is reads that span real file and data"""
        ret = ""
        if self.offset < self.size:
            s = self.fp.read(count)
            ret = s
            self.offset += len(s)
            if count > 0:
                count -= len(s)
        if count != 0:
            doff = self.offset - self.size
            self.data.insert(0, "".join(self.data))
            del self.data[1:]
            s = self.data[0][doff : doff + count]
            self.offset += len(s)
            ret += s
        return ret

    def write(self, s):
        self.data.append(bytes(s))
        self.offset += len(s)
        self._end += len(s)


def _divertopener(opener, target):
    """build an opener that writes in 'target.a' instead of 'target'"""

    def _divert(name, mode="r", checkambig=False):
        if name != target:
            return opener(name, mode)
        return opener(name + ".a", mode)

    return _divert


def _delayopener(opener, target, buf):
    """build an opener that stores chunks in 'buf' instead of 'target'"""

    def _delay(name, mode="r", checkambig=False):
        if name != target:
            return opener(name, mode)
        return appender(opener, name, mode, buf)

    return _delay


@attr.s
class _changelogrevision(object):
    # Extensions might modify _defaultextra, so let the constructor below pass
    # it in
    extra = attr.ib()
    manifest = attr.ib(default=nullid)
    user = attr.ib(default="")
    date = attr.ib(default=(0, 0))
    files = attr.ib(default=attr.Factory(list))
    description = attr.ib(default="")


class changelogrevision(object):
    """Holds results of a parsed changelog revision.

    Changelog revisions consist of multiple pieces of data, including
    the manifest node, user, and date. This object exposes a view into
    the parsed object.
    """

    __slots__ = (u"_offsets", u"_text", u"_files")

    def __new__(cls, text):
        if not text:
            return _changelogrevision(extra=_defaultextra)

        self = super(changelogrevision, cls).__new__(cls)
        # We could return here and implement the following as an __init__.
        # But doing it here is equivalent and saves an extra function call.

        # format used:
        # nodeid\n        : manifest node in ascii
        # user\n          : user, no \n or \r allowed
        # time tz extra\n : date (time is int or float, timezone is int)
        #                 : extra is metadata, encoded and separated by '\0'
        #                 : older versions ignore it
        # files\n\n       : files modified by the cset, no \n or \r allowed
        # (.*)            : comment (free text, ideally utf-8)
        #
        # changelog v0 doesn't use extra

        nl1 = text.index(b"\n")
        nl2 = text.index(b"\n", nl1 + 1)
        nl3 = text.index(b"\n", nl2 + 1)

        # The list of files may be empty. Which means nl3 is the first of the
        # double newline that precedes the description.
        if text[nl3 + 1 : nl3 + 2] == b"\n":
            doublenl = nl3
        else:
            doublenl = text.index(b"\n\n", nl3 + 1)

        self._offsets = (nl1, nl2, nl3, doublenl)
        self._text = text
        self._files = None

        return self

    @property
    def manifest(self):
        return bbin(self._text[0 : self._offsets[0]])

    @property
    def user(self):
        off = self._offsets
        return encoding.tolocalstr(self._text[off[0] + 1 : off[1]])

    @property
    def _rawdate(self):
        off = self._offsets
        dateextra = self._text[off[1] + 1 : off[2]]
        return dateextra.split(b" ", 2)[0:2]

    @property
    def _rawextra(self):
        off = self._offsets
        dateextra = self._text[off[1] + 1 : off[2]]
        fields = dateextra.split(b" ", 2)
        if len(fields) != 3:
            return None

        return fields[2]

    @property
    def date(self):
        raw = self._rawdate
        time = float(raw[0])
        # Various tools did silly things with the timezone.
        try:
            timezone = int(raw[1])
        except ValueError:
            timezone = 0

        return time, timezone

    @property
    def extra(self):
        raw = self._rawextra
        if raw is None:
            return _defaultextra

        return decodeextra(raw)

    @property
    def files(self):
        if self._files is not None:
            return self._files

        off = self._offsets
        if off[2] == off[3]:
            self._files = tuple()
        else:
            self._files = tuple(decodeutf8(self._text[off[2] + 1 : off[3]]).split("\n"))
        return self._files

    @property
    def description(self):
        return encoding.tolocalstr(self._text[self._offsets[3] + 2 :])


class changelog(revlog.revlog):
    def __init__(self, opener, uiconfig, trypending=False, zstore=None):
        """Load a changelog revlog using an opener.

        If ``trypending`` is true, we attempt to load the index from a
        ``00changelog.i.a`` file instead of the default ``00changelog.i``.
        The ``00changelog.i.a`` file contains index (and possibly inline
        revision) data for a transaction that hasn't been finalized yet.
        It exists in a separate file to facilitate readers (such as
        hooks processes) accessing data before a transaction is finalized.
        """
        self._uiconfig = uiconfig
        self._visibleheads = self._loadvisibleheads(opener)
        bypasstransaction = bool(
            getattr(opener, "options", {}).get("bypass-revlog-transaction")
        )
        if trypending and not bypasstransaction and opener.exists("00changelog.i.a"):
            indexfile = "00changelog.i.a"
        else:
            indexfile = "00changelog.i"

        if uiconfig.configbool("experimental", "rust-commits") and bypasstransaction:
            # self.inner: The Rust object that handles all changelog
            # operations. Currently it is used in parallel with the
            # existing revlog logic. Eventually it will replace all
            # operations here with modern setup.
            self.inner = bindings.dag.commits.openrevlog(opener.join(""))
        else:
            self.inner = None
        self._userustcache = {}

        datafile = "00changelog.d"
        revlog.revlog.__init__(
            self,
            opener,
            indexfile,
            datafile=datafile,
            checkambig=True,
            mmaplargeindex=True,
            index2=not self.userust("index2"),
        )

        if self._initempty:
            # changelogs don't benefit from generaldelta
            self.version &= ~revlog.FLAG_GENERALDELTA
            self._generaldelta = False
            # format.inline-changelog is used by tests
            if not uiconfig.configbool("format", "inline-changelog"):
                # disable inline to make it easier to read changelog.i
                self.version &= ~revlog.FLAG_INLINE_DATA
                self._inline = False

        # Delta chains for changelogs tend to be very small because entries
        # tend to be small and don't delta well with each. So disable delta
        # chains.
        self.storedeltachains = False

        self._realopener = opener
        self._delayed = False
        self._delaybuf = None
        self._divert = False

        if uiconfig.configbool("format", "use-zstore-commit-data-revlog-fallback"):
            self._zstorefallback = "revlog"
        elif uiconfig.configbool("format", "use-zstore-commit-data-server-fallback"):
            self._zstorefallback = "server"
        else:
            self._zstorefallback = None

        self.zstore = zstore

    def userust(self, name):
        """Test whether to use rust-commit structure for a particular job

        Eventually (after migrating narrow-heads repos, and killing hgsql) this
        should always return True.
        """
        cache = self._userustcache
        value = cache.get(name)
        if value is None:
            if self.inner is None:
                value = False
            else:
                value = self._uiconfig.configbool(
                    "experimental", "rust-commits:%s" % name
                )
            cache[name] = value
        return value

    @property
    def dag(self):
        """Get the DAG with algorithms. Require rust-commit."""
        inner = self.inner
        if inner is None:
            return None
        return inner.dagalgo()

    def dageval(self, func, extraenv=None):
        """Evaluate func with the current DAG context.

        Example:

            cl.dageval(lambda: roots(ancestors([x])))

        is equivalent to:

            dag = cl.dag
            dag.roots(dag.ancestors([x]))

        'extraenv' is optionally a dict. It sets extra names. For example,
        providing revset-like functions like `draft`, `public`, `bookmarks`.
        """
        env = dict(func.func_globals)
        dag = self.dag
        for name in func.func_code.co_names:
            # name is potentially a string used by LOAD_GLOBAL bytecode.
            if extraenv is not None and name in extraenv:
                # Provided by 'extraenv'.
                env[name] = extraenv[name]
            else:
                # Provided by 'dag'.
                value = getattr(dag, name, None)
                if value is not None:
                    env[name] = value
        code = func.func_code
        argdefs = func.func_defaults
        freevars = func.func_closure
        # Create a new function that uses the same logic except for different
        # env (globals).
        newfunc = type(func)(code, env, argdefs, freevars)
        return newfunc()

    @property
    def idmap(self):
        """Get the IdMap. Require rust-commit."""
        return self.inner.idmap()

    @property
    def torevs(self):
        """Convert a Set using commit hashes to an IdSet using numbers

        The Set is usually obtained via `self.dag` APIs.
        """
        return self.inner.torevs

    @property
    def tonodes(self):
        """Convert an IdSet to Set. The reverse of torevs."""
        return self.inner.tonodes

    def _loadvisibleheads(self, opener):
        return visibility.visibleheads(opener)

    def tip(self):
        # type: () -> bytes
        """filtered version of revlog.tip"""
        if self.userust("tip"):
            return self.dag.all().first() or nullid
        for i in range(len(self) - 1, -2, -1):
            # pyre-fixme[7]: Expected `bytes` but got implicit return value of `None`.
            return self.node(i)

    def __contains__(self, rev):
        """filtered version of revlog.__contains__"""
        if self.userust("contains"):
            return rev is not None and rev in self.torevs(self.dag.all())
        else:
            return rev is not None and 0 <= rev < len(self)

    def __iter__(self):
        """filtered version of revlog.__iter__"""
        if self.userust("iter"):
            return self.torevs(self.dag.all()).iterasc()
        else:
            return revlog.revlog.__iter__(self)

    def __len__(self):
        if self.userust("len"):
            return len(self.dag.all())
        else:
            return super(changelog, self).__len__()

    def revs(self, start=0, stop=None):
        """filtered version of revlog.revs"""
        if self.userust("revs"):
            allrevs = self.torevs(self.dag.all())
            if stop is not None:
                # exclusive -> inclusive
                stop = stop - 1
            revs = bindings.dag.spans.unsaferange(start, stop) & allrevs
            for i in revs.iterasc():
                yield i
            return

        for i in super(changelog, self).revs(start, stop):
            yield i

    @property
    def nodemap(self):
        if self.userust("nodemap"):
            # self.idmap might change, do not cache this nodemap result
            return nodemap(self)
        else:
            self.rev(self.node(0))
            return self._nodecache

    @nodemap.setter
    def nodemap(self, value):
        if self.userust("nodemap"):
            pass
        else:
            self._nodecache = value

    def reachableroots(self, minroot, heads, roots, includepath=False):
        if self.userust("reachableroots"):
            tonodes = self.tonodes
            headnodes = tonodes(heads)
            rootnodes = tonodes(roots)
            dag = self.dag
            # special case: null::X -> ::X
            if len(rootnodes) == 0 and nullrev in roots:
                nodes = dag.ancestors(headnodes)
            else:
                nodes = dag.range(rootnodes, headnodes)
            if not includepath:
                nodes = nodes & rootnodes
                # The old code path with includepath=False filters "roots"
                # out. Emulate that filtering by headsancestors.
                # It has subtle differences, though. See
                # test-log-filenode-conflict.t change of this commit.
                nodes = dag.headsancestors(nodes)
            return list(self.torevs(nodes))
        else:
            return self.index.reachableroots2(minroot, heads, roots, includepath)

    def heads(self, start=None, stop=None):
        raise error.ProgrammingError(
            "do not use changelog.heads, use repo.heads instead"
        )

    def headrevs(self):
        raise error.ProgrammingError(
            "do not use changelog.headrevs, use repo.headrevs instead"
        )

    def rawheadrevs(self):
        """Raw heads that exist in the changelog.
        This bypasses the visibility layer.

        This is currently only used by discovery. The revlog streamclone will
        get the revlog changelog first without remote bookmarks, followed by a
        pull. The pull needs to use all heads since remote bookmarks are not
        available at that time. If streamclone can provide both the DAG and the
        heads in a consistent way, then discovery can just use references as
        heads isntead.
        """
        if self.userust("rawheadrevs"):
            dag = self.dag
            heads = dag.headsancestors(dag.all())
            # Be compatible with C index headrevs: Return in ASC order.
            revs = self.torevs(heads)
            return list(revs.iterasc())
        else:
            return self.index.headrevs()

    def strip(self, minlink, transaction):
        # Invalidate on-disk nodemap.
        if self.indexfile.startswith("00changelog"):
            self.opener.tryunlink("00changelog.nodemap")
            self.opener.tryunlink("00changelog.i.nodemap")
        if self.userust("strip"):
            self.inner.strip([self.node(minlink)])
        else:
            super(changelog, self).strip(minlink, transaction)

    def rev(self, node):
        """filtered version of revlog.rev"""
        if self.userust("rev"):
            if node == wdirid:
                raise error.WdirUnsupported
            try:
                return self.idmap.node2id(node)
            except error.CommitLookupError:
                raise error.LookupError(node, self.indexfile, _("no node"))
        r = super(changelog, self).rev(node)
        return r

    def node(self, rev):
        """filtered version of revlog.node"""
        if self.userust("node"):
            if rev == wdirrev:
                raise error.WdirUnsupported
            try:
                return self.idmap.id2node(rev)
            except error.CommitLookupError:
                raise IndexError("revlog index out of range")
        else:
            return super(changelog, self).node(rev)

    def linkrev(self, rev):
        """filtered version of revlog.linkrev"""
        return super(changelog, self).linkrev(rev)

    def parentrevs(self, rev):
        """filtered version of revlog.parentrevs"""
        return super(changelog, self).parentrevs(rev)

    def flags(self, rev):
        """filtered version of revlog.flags"""
        return super(changelog, self).flags(rev)

    def delayupdate(self, tr):
        "delay visibility of index updates to other readers"
        if self._bypasstransaction:
            return

        if not self._delayed:
            if len(self) == 0:
                self._divert = True
                if self._realopener.exists(self.indexfile + ".a"):
                    self._realopener.unlink(self.indexfile + ".a")
                self.opener = _divertopener(self._realopener, self.indexfile)
            else:
                self._delaybuf = []
                self.opener = _delayopener(
                    self._realopener, self.indexfile, self._delaybuf
                )
        self._delayed = True
        tr.addpending("cl-%i" % id(self), self._writepending, onetime=True)
        tr.addfinalize("cl-%i" % id(self), self._finalize)

    def _finalize(self, tr):
        "finalize index updates"
        if self._bypasstransaction:
            return
        self._delayed = False
        self.opener = self._realopener
        # move redirected index data back into place
        if self._divert:
            assert not self._delaybuf
            tmpname = self.indexfile + ".a"
            nfile = self.opener.open(tmpname)
            nfile.close()
            self.opener.rename(tmpname, self.indexfile, checkambig=True)
        elif self._delaybuf:
            fp = self.opener(self.indexfile, "a", checkambig=True)
            fp.write(b"".join(self._delaybuf))
            fp.close()
            self._delaybuf = None
        self._divert = False
        # split when we're done
        self.checkinlinesize(tr)

    def _writepending(self, tr):
        "create a file containing the unfinalized state for pretxnchangegroup"
        assert not self._bypasstransaction
        if self._delaybuf:
            # make a temporary copy of the index
            fp1 = self._realopener(self.indexfile)
            pendingfilename = self.indexfile + ".a"
            # register as a temp file to ensure cleanup on failure
            tr.registertmp(pendingfilename)
            # write existing data
            fp2 = self._realopener(pendingfilename, "w")
            fp2.write(fp1.read())
            # add pending data
            fp2.write(b"".join(self._delaybuf))
            fp2.close()
            # switch modes so finalize can simply rename
            self._delaybuf = None
            self._divert = True
            self.opener = _divertopener(self._realopener, self.indexfile)

        if self._divert:
            return True

        return False

    def checkinlinesize(self, tr, fp=None):
        if not self._delayed:
            revlog.revlog.checkinlinesize(self, tr, fp)

    def read(self, node):
        """Obtain data from a parsed changelog revision.

        Returns a 6-tuple of:

           - manifest node in binary
           - author/user as a localstr
           - date as a 2-tuple of (time, timezone)
           - list of files
           - commit message as a localstr
           - dict of extra metadata

        Unless you need to access all fields, consider calling
        ``changelogrevision`` instead, as it is faster for partial object
        access.
        """
        c = changelogrevision(self.revision(node))
        return (c.manifest, c.user, c.date, c.files, c.description, c.extra)

    def changelogrevision(self, nodeorrev):
        """Obtain a ``changelogrevision`` for a node or revision."""
        return changelogrevision(self.revision(nodeorrev))

    def readfiles(self, node):
        """
        short version of read that only returns the files modified by the cset
        """
        text = self.revision(node)
        return readfiles(text)

    def add(
        self, manifest, files, desc, transaction, p1, p2, user, date=None, extra=None
    ):
        text = hgcommittext(manifest, files, desc, user, date, extra)
        btext = encodeutf8(text)
        if self.userust("addrevision"):
            node = revlog.hash(btext, p1, p2)
            parents = [p for p in (p1, p2) if p != nullid]
            self.inner.addcommits([(node, parents, btext)])
            nodes = transaction.changes.get("nodes")
            if nodes is not None:
                nodes.append(node)
            return node

        result = self.addrevision(btext, transaction, len(self), p1, p2)

        zstore = self.zstore
        if zstore is not None:
            zstore.flush()
        return result

    def addgroup(self, deltas, linkmapper, transaction, addrevisioncb=None):
        if self.userust("addrevision"):
            nodes = []
            for node, p1, p2, linknode, deltabase, delta, flags in deltas:
                assert flags == 0, "changelog flags cannot be non-zero"
                parents = [p for p in (p1, p2) if p != nullid]
                basetext = self.revision(deltabase)
                rawtext = bytes(mdiff.patch(basetext, delta))
                self.inner.addcommits([(node, parents, rawtext)])
                if addrevisioncb:
                    addrevisioncb(self, node)
                nodes.append(node)
            trnodes = transaction.changes.get("nodes")
            if trnodes is not None:
                trnodes += nodes
            return nodes
        result = super(changelog, self).addgroup(
            deltas, linkmapper, transaction, addrevisioncb
        )
        zstore = self.zstore
        if zstore is not None:
            zstore.flush()
        return result

    def branchinfo(self, rev):
        """return the branch name and open/close state of a revision

        This function exists because creating a changectx object
        just to access this is costly."""
        extra = self.read(rev)[5]
        return encoding.tolocal(extra.get("branch")), "close" in extra

    def _addrevision(
        self,
        node,
        rawtext,
        transaction,
        link,
        p1,
        p2,
        flags,
        cachedelta,
        ifh,
        dfh,
        **kwargs
    ):
        if self.userust("addrevision"):
            if rawtext is None:
                baserev, delta = cachedelta
                basetext = self.revision(baserev, _df=dfh, raw=False)
                rawtext = bytes(mdiff.patch(basetext, delta))
            parents = [p for p in (p1, p2) if p != nullid]
            self.inner.addcommits([(node, parents, rawtext)])
            return node
        # overlay over the standard revlog._addrevision to track the new
        # revision on the transaction.
        rev = len(self)
        node = super(changelog, self)._addrevision(
            node,
            rawtext,
            transaction,
            link,
            p1,
            p2,
            flags,
            cachedelta,
            ifh,
            dfh,
            **kwargs
        )

        # Also write (key=node, data=''.join(sorted([p1,p2]))+text) to zstore
        # if zstore exists.
        # `_addrevision` is the single API that writes to revlog `.d`.
        # It is used by `add` and `addgroup`.
        zstore = self.zstore
        if zstore is not None:
            # `rawtext` can be None (code path: revlog.addgroup), in that case
            # `cachedelta` is the way to get `text`.
            if rawtext is None:
                baserev, delta = cachedelta
                basetext = self.revision(baserev, _df=dfh, raw=False)
                text = rawtext = mdiff.patch(basetext, delta)
            else:
                # text == rawtext only if there is no flags.
                # We need 'text' to calculate commit SHA1.
                assert not flags, "revlog flags on changelog is unexpected"
                text = rawtext
            sha1text = textwithheader(text, p1, p2)
            # Use `p1` as a potential delta-base.
            zstorenode = zstore.insert(sha1text, [p1])
            assert zstorenode == node, "zstore SHA1 should match node"

        inner = self.inner
        if inner is not None:
            if rawtext is None:
                baserev, delta = cachedelta
                basetext = self.revision(baserev, _df=dfh, raw=False)
                rawtext = mdiff.patch(basetext, delta)
            parentnodes = [p for p in (p1, p2) if p != nullid]
            inner.addcommits([(node, parentnodes, bytes(rawtext))])

        nodes = transaction.changes.get("nodes")
        if nodes is not None:
            nodes.append(node)
        return node

    def revision(self, nodeorrev, _df=None, raw=False):
        if self.userust("revision"):
            if nodeorrev in {nullid, nullrev}:
                return b""
            if isinstance(nodeorrev, int):
                node = self.node(nodeorrev)
            else:
                node = nodeorrev
            text = self.inner.getcommitrawtext(node)
            if text is None:
                raise error.LookupError(node, self.indexfile, _("no node"))
            return text

        # type: (Union[int, bytes], Optional[IO], bool) -> bytes
        # "revision" is the single API that reads `.d` from revlog.
        # Use zstore if possible.
        zstore = self.zstore
        if zstore is None:
            return super(changelog, self).revision(nodeorrev, _df=_df, raw=raw)
        else:
            if isinstance(nodeorrev, int):
                node = self.node(nodeorrev)
            else:
                node = nodeorrev
            if node == nullid:
                return b""
            text = zstore[node]
            if text is None:
                # fallback to revlog
                if self._zstorefallback == "revlog":
                    return super(changelog, self).revision(nodeorrev, _df=_df, raw=raw)
                raise error.LookupError(node, self.indexfile, _("no data for node"))
            # Strip the p1, p2 header
            return text[40:]

    def nodesbetween(self, roots, heads):
        """Calculate (roots::heads, roots & (roots::heads), heads & (roots::heads))"""
        if self.userust("nodesbetween"):
            result = self.dag.range(roots, heads)
            roots = roots & result
            heads = heads & result
            # Return in ASC order to be compatible with the old logic.
            return list(result.iterrev()), list(roots.iterrev()), list(heads.iterrev())
        else:
            return super(changelog, self).nodesbetween(roots, heads)

    def children(self, node):
        """Return children(node)"""
        if self.userust("children"):
            nodes = self.dag.children([node])
            return list(nodes)
        else:
            return super(changelog, self).children(node)

    def descendants(self, revs):
        """Return ((revs::) - roots(revs)) in revs."""
        if self.userust("descendants"):
            dag = self.dag
            # nullrev special case.
            if nullrev in revs:
                result = dag.all()
            else:
                nodes = self.tonodes(revs)
                result = dag.descendants(nodes) - dag.roots(nodes)
            for rev in self.torevs(result).iterasc():
                yield rev
        else:
            for rev in super(changelog, self).descendants(revs):
                yield rev

    def findcommonmissing(self, common, heads):
        """Return (torevs(::common), (::heads) - (::common))"""
        if self.userust("findcommonmissing"):
            # "::heads - ::common" is "heads % common", aka. the "only"
            # operation.
            onlyheads, commonancestors = self.dag.onlyboth(heads, common)
            # commonancestors can be large, do not convert to list
            return self.torevs(commonancestors), list(onlyheads.iterrev())
        else:
            return super(changelog, self).findcommonmissing(common, heads)

    def isancestor(self, a, b):
        """Test if a (in node) is an ancestor of b (in node)"""
        if self.userust("isancestor"):
            if a == nullid or b == nullid:
                return False
            return self.dag.isancestor(a, b)
        else:
            return super(changelog, self).isancestor(a, b)

    def ancestor(self, a, b):
        """Return the common ancestor, or nullid if there are no common
        ancestors.

        Common ancestors are defined as heads(::a & ::b).

        When there are multiple common ancestors, a "random" one is returned.
        """
        if self.userust("ancestor"):
            if nullid == a or nullid == b:
                return nullid
            return self.dag.gcaone([a, b]) or nullid
        else:
            return super(changelog, self).ancestor(a, b)

    def descendant(self, start, end):
        """Test if start (in rev) is an ancestor of end (in rev)"""
        return self.isancestor(self.node(start), self.node(end))

    def _partialmatch(self, hexprefix):
        if self.userust("partialmatch"):
            matched = self.idmap.hexprefixmatch(hexprefix)
            if len(matched) > 1:
                # TODO: Add hints about possible matches.
                raise error.LookupError(
                    hexprefix, self.indexfile, _("ambiguous identifier")
                )
            elif len(matched) == 1:
                return matched[0]
            else:
                return None
        else:
            return super(changelog, self)._partialmatch(hexprefix)

    def ancestors(self, revs, stoprev=0, inclusive=False):
        """Return ::revs (in revs) if inclusive is True.

        If inclusive is False, return ::parents(revs).
        If stoprev is not zero, filter the result.
        stoprev is ignored in the Rust implementation.
        """
        if self.userust("ancestors"):
            nodes = self.tonodes(revs)
            dag = self.dag
            if not inclusive:
                nodes = dag.parents(nodes)
            ancestornodes = dag.ancestors(nodes)
            return self.torevs(ancestornodes)
        else:
            return super(changelog, self).ancestors(revs, stoprev, inclusive)

    def commonancestorsheads(self, a, b):
        """Return heads(::a & ::b)"""
        if self.userust("commonancestorsheads"):
            # null special case
            if nullid == a or nullid == b:
                return []
            return list(self.dag.gcaall([a, b]))
        else:
            return super(changelog, self).commonancestorsheads(a, b)

    def parents(self, node):
        if self.userust("parents"):
            # special case for null
            if node == nullid:
                return (nullid, nullid)
            parents = list(self.dag.parentnames(node))
            while len(parents) < 2:
                parents.append(nullid)
            return parents
        else:
            return super(changelog, self).parents(node)

    def parentrevs(self, rev):
        if self.userust("parents"):
            return list(map(self.rev, self.parents(self.node(rev))))
        else:
            return super(changelog, self).parentrevs(rev)

    def deltaparent(self, rev):
        if self.userust("bypassrevlog"):
            # Changelog does not have deltaparent
            return nullrev
        else:
            return super(changelog, self).deltaparent(rev)

    def flags(self, rev):
        if self.userust("bypassrevlog"):
            # Changelog does not have deltaparent
            return 0
        else:
            return super(changelog, self).flags(rev)


class nodemap(object):
    def __init__(self, changelog):
        self.changelog = changelog

    def __getitem__(self, node):
        rev = self.get(node)
        if rev is None:
            raise error.RevlogError(_("cannot find rev for %s") % hex(node))
        else:
            return rev

    def __setitem__(self, node, rev):
        if self.get(node) != rev:
            raise error.ProgrammingError("nodemap by Rust DAG is immutable")

    def __delitem__(self, node):
        raise error.ProgrammingError("nodemap by Rust DAG is immutable")

    def __contains__(self, node):
        return node in self.changelog.idmap

    def get(self, node, default=None):
        idmap = self.changelog.idmap
        if node not in idmap:
            return default
        else:
            return idmap.node2id(node)

    def destroying(self):
        pass


def readfiles(text):
    # type: (bytes) -> List[str]
    """
    >>> from .pycompat import bytechr as chr
    >>> d = {'nl': chr(10)}
    >>> withfiles = b'commitnode%(nl)sAuthor%(nl)sMetadata and extras%(nl)sfile1%(nl)sfile2%(nl)sfile3%(nl)s%(nl)s' % d
    >>> readfiles(withfiles)
    ['file1', 'file2', 'file3']
    >>> withoutfiles = b'commitnode%(nl)sAuthor%(nl)sMetadata and extras%(nl)s%(nl)sCommit summary%(nl)s%(nl)sCommit description%(nl)s' % d
    >>> readfiles(withoutfiles)
    []
    """
    if not text:
        return []

    first = 0
    last = text.index(b"\n\n")

    n = 3
    while n != 0:
        try:
            first = text.index(b"\n", first, last) + 1
        except ValueError:
            return []
        n -= 1

    return decodeutf8(text[first:last]).split("\n")


def hgcommittext(manifest, files, desc, user, date, extra):
    """Generate the 'text' of a commit"""
    # Convert to UTF-8 encoded bytestrings as the very first
    # thing: calling any method on a localstr object will turn it
    # into a str object and the cached UTF-8 string is thus lost.
    user, desc = encoding.fromlocal(user), encoding.fromlocal(desc)

    user = user.strip()
    # An empty username or a username with a "\n" will make the
    # revision text contain two "\n\n" sequences -> corrupt
    # repository since read cannot unpack the revision.
    if not user:
        raise error.RevlogError(_("empty username"))
    if "\n" in user:
        raise error.RevlogError(_("username %s contains a newline") % repr(user))

    desc = stripdesc(desc)

    if date:
        parseddate = "%d %d" % util.parsedate(date)
    else:
        parseddate = "%d %d" % util.makedate()
    if extra:
        branch = extra.get("branch")
        if branch in ("default", ""):
            del extra["branch"]
        elif branch in (".", "null", "tip"):
            raise error.RevlogError(_("the name '%s' is reserved") % branch)
    if extra:
        extra = encodeextra(extra)
        parseddate = "%s %s" % (parseddate, extra)
    l = [hex(manifest), user, parseddate] + sorted(files) + ["", desc]
    text = "\n".join(l)
    return text

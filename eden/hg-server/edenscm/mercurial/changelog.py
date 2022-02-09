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
from .pycompat import decodeutf8, encodeutf8, iteritems, range, isint
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


def decodeextra(text: bytes) -> "Dict[str, str]":
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

    __slots__ = ("_offsets", "_text", "_files")

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

        datafile = "00changelog.d"
        revlog.revlog.__init__(
            self,
            opener,
            indexfile,
            datafile=datafile,
            checkambig=True,
            mmaplargeindex=True,
            index2=True,
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

    def userust(self, name=None):
        return False

    @property
    def algorithmbackend(self):
        return "revlog-cpy"

    @property
    def dag(self):
        """Get the DAG with algorithms. Require rust-commit."""
        return None

    def _loadvisibleheads(self, opener):
        return visibility.visibleheads(opener)

    def tip(self) -> bytes:
        """filtered version of revlog.tip"""
        for i in range(len(self) - 1, -2, -1):
            # pyre-fixme[7]: Expected `bytes` but got implicit return value of `None`.
            return self.node(i)

    def __contains__(self, rev):
        """filtered version of revlog.__contains__"""
        return rev is not None and 0 <= rev < len(self)

    def revs(self, start=0, stop=None):
        """filtered version of revlog.revs"""
        for i in super(changelog, self).revs(start, stop):
            yield i

    @property
    def nodemap(self):
        self.rev(self.node(0))
        return self._nodecache

    @nodemap.setter
    def nodemap(self, value):
        self._nodecache = value

    def reachableroots(self, minroot, heads, roots, includepath=False):
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
        return self.index.headrevs()

    def strip(self, minlink, transaction):
        # Invalidate on-disk nodemap.
        if self.indexfile.startswith("00changelog"):
            self.opener.tryunlink("00changelog.nodemap")
            self.opener.tryunlink("00changelog.i.nodemap")
        super(changelog, self).strip(minlink, transaction)

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
        self._invalidaterustrevlog()
        text = hgcommittext(manifest, files, desc, user, date, extra)
        result = self.addrevision(text, transaction, len(self), p1, p2)

        zstore = self.zstore
        if zstore is not None:
            zstore.flush()
        return result

    def addgroup(self, deltas, linkmapper, transaction):
        self._invalidaterustrevlog()
        result = super(changelog, self).addgroup(deltas, linkmapper, transaction)
        zstore = self.zstore
        if zstore is not None:
            zstore.flush()
        return result

    def _invalidaterustrevlog(self):
        """Remove the 00changelog.len metadata used by the Rust revlog.
        Call this function whenever the revlog is changed by Python.
        """
        svfs = self._realopener
        svfs.tryunlink("00changelog.len")
        # Reset 'tip'
        if util.safehasattr(svfs, "metalog"):
            svfs.metalog.remove("tip")

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
        **kwargs,
    ):
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
            **kwargs,
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

        nodes = transaction.changes.get("nodes")
        if nodes is not None:
            nodes.append(node)
        return node

    def revision(
        self,
        nodeorrev: "Union[int, bytes]",
        _df: "Optional[IO]" = None,
        raw: bool = False,
    ) -> bytes:
        # "revision" is the single API that reads `.d` from revlog.
        # Use zstore if possible.
        zstore = self.zstore
        if zstore is None:
            return super(changelog, self).revision(nodeorrev, _df=_df, raw=raw)
        else:
            if isint(nodeorrev):
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


def readfiles(text: bytes) -> "List[str]":
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
    text = encodeutf8("\n".join(l), errors="surrogateescape")
    return text

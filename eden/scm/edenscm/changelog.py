# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# changelog.py - changelog class for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import subprocess
from typing import Dict, List, Optional

from . import encoding, error, gituser, revlog, util
from .i18n import _
from .node import bbin, hex, nullid
from .pycompat import decodeutf8, encodeutf8, iteritems
from .thirdparty import attr


_defaultextra = {"branch": "default"}

textwithheader = revlog.textwithheader


def _string_escape(text):
    """
    >>> d = {'nl': chr(10), 'bs': chr(92), 'cr': chr(13), 'nul': chr(0)}
    >>> s = "ab%(nl)scd%(bs)s%(bs)sn%(nul)sab%(cr)scd%(bs)s%(nl)s" % d
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
    >>> sorted(decodeextra(encodeextra({'foo': 'bar', 'baz': '\\x002'}).encode("utf-8")
    ...                    ).items())
    [('baz', '\\x002'), ('branch', 'default'), ('foo', 'bar')]
    >>> sorted(decodeextra(encodeextra({'foo': 'bar',
    ...                                 'baz': '\\x5c\\x002'}).encode("utf-8")
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


def readfiles(text: bytes) -> "List[str]":
    """
    >>> d = {'nl': chr(10)}
    >>> withfiles = 'commitnode%(nl)sAuthor%(nl)sMetadata and extras%(nl)sfile1%(nl)sfile2%(nl)sfile3%(nl)s%(nl)s' % d
    >>> readfiles(withfiles.encode("utf8"))
    ['file1', 'file2', 'file3']
    >>> withoutfiles = 'commitnode%(nl)sAuthor%(nl)sMetadata and extras%(nl)s%(nl)sCommit summary%(nl)s%(nl)sCommit description%(nl)s' % d
    >>> readfiles(withoutfiles.encode("utf8"))
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


def gitdatestr(datestr: str) -> str:
    """convert datestr to git date str used in commits

    >>> util.parsedate('2000-01-01T00:00:00 +0700')
    (946659600, -25200)
    >>> gitdatestr('2000-01-01T00:00:00 +0700')
    '946659600 +0700'
    >>> gitdatestr('2000-01-01T00:00:00 +0000')
    '946684800 +0000'
    """
    utc, offset = util.parsedate(datestr)
    if offset > 0:
        offsetsign = "-"
    else:
        offsetsign = "+"
        offset = -offset
    offset = offset // 60
    offsethour = offset // 60
    offsetminute = offset % 60
    return "%d %s%02d%02d" % (utc, offsetsign, offsethour, offsetminute)


def gitcommittext(
    tree: bytes,
    parents: List[bytes],
    desc: str,
    user: str,
    date: str,
    extra: Optional[Dict[str, str]],
    gpgkeyid: Optional[str] = None,
) -> bytes:
    r"""construct raw text (bytes) used by git commit

    If a gpgkeyid is specified, `gpg` will use it to create a signature for
    the unsigned commit object. This signature will be included in the commit
    text exactly as it would in Git.

    Note that while Git supports multiple signature formats (openpgp, x509, ssh),
    Sapling only supports openpgp today.

    >>> import binascii
    >>> tree = binascii.unhexlify('deadbeef')
    >>> desc = " HI! \n   another line with leading spaces\n\nsecond line\n\n\n"
    >>> user = "Alyssa P. Hacker <alyssa@example.com>"
    >>> date = "2000-01-01T00:00:00 +0700"
    >>> no_parents = gitcommittext(tree, [], desc, user, date, None)
    >>> no_parents == (
    ...     b'tree deadbeef\n' +
    ...     b'author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'\n' +
    ...     b' HI!\n' +
    ...     b'   another line with leading spaces\n' +
    ...     b'\n' +
    ...     b'second line\n'
    ... )
    True
    >>> p1 = binascii.unhexlify('deadc0de')
    >>> one_parent = gitcommittext(tree, [p1], desc, user, date, None)
    >>> one_parent == (
    ...     b'tree deadbeef\n' +
    ...     b'parent deadc0de\n' +
    ...     b'author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'\n' +
    ...     b' HI!\n' +
    ...     b'   another line with leading spaces\n' +
    ...     b'\n' +
    ...     b'second line\n'
    ... )
    True
    >>> p2 = binascii.unhexlify('baadf00d')
    >>> two_parents = gitcommittext(tree, [p1, p2], desc, user, date, None)
    >>> two_parents == (
    ...     b'tree deadbeef\n' +
    ...     b'parent deadc0de\n' +
    ...     b'parent baadf00d\n' +
    ...     b'author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'\n' +
    ...     b' HI!\n' +
    ...     b'   another line with leading spaces\n' +
    ...     b'\n' +
    ...     b'second line\n'
    ... )
    True
    """
    # Example:
    # tree 97e8739f1945a4ba78c9bc1c670718c5dc5c08eb
    # parent 402aab067c4f60fa8ed4868e76b54064fa06a245
    # author svcscm svcscm <svcscm@fb.com> 1626293346 -0700
    # committer Facebook GitHub Bot <facebook-github-bot@users.noreply.github.com> 1626293437 -0700
    #
    # Updating submodules
    committer = (extra.get("committer") if extra else None) or user
    committerdate = (extra.get("committer_date") if extra else None) or date
    parent_entries = "".join(f"parent {hex(p)}\n" for p in parents)
    pre_sig_text = f"""\
tree {hex(tree)}
{parent_entries}author {gituser.normalize(user)} {gitdatestr(date)}
committer {gituser.normalize(committer)} {gitdatestr(committerdate)}"""

    normalized_desc = stripdesc(desc)
    text = pre_sig_text + f"\n\n{normalized_desc}\n"
    text = encodeutf8(text, errors="surrogateescape")
    if not gpgkeyid:
        return text

    # This should match how Git signs commits:
    # https://github.com/git/git/blob/2e71cbbddd64695d43383c25c7a054ac4ff86882/gpg-interface.c#L956-L960
    # Long-form arguments for `gpg` are used for clarity.
    sig_bytes = subprocess.check_output(
        [
            # Should the path to gpg be configurable?
            "gpg",
            "--status-fd=2",
            "--detach-sign",
            "--sign",
            "--armor",
            "--local-user",
            gpgkeyid,
        ],
        stderr=subprocess.DEVNULL,
        input=text,
    )
    return _signedgitcommittext(sig_bytes, pre_sig_text, normalized_desc, gpgkeyid)


def _signedgitcommittext(
    sig_bytes: bytes, pre_sig_text: str, normalized_desc: str, gpgkeyid: str
) -> bytes:
    r"""produces a signed commit from the intermediate values produced by gitcommittext()

    >>> sig_bytes = (
    ...     b"-----BEGIN PGP SIGNATURE-----\r\n" +
    ...     b"\r\n" +
    ...     b"iQEzBAABCAAdFiEEurYkrcQEDEhXMjb8tXeqdrrlBbEFAmOPpRIACgkQtXeqdrrl\r\n" +
    ...     b"BbE8hAf/eybgd1jrovZhs8X/SU2UO4rQnekz5D1BpAVjKUIDTfvuVg7sczTyuXvE\r\n" +
    ...     b"pkuhkeZd2Is0HvSzWa9dD88VECrwQfHjOFe2Ffb7QdVN4811pZ4+lcGcWKKVG9Oq\r\n" +
    ...     b"uAtXJgXpBf58Vp9x7wgnbqPFlSUTk5vlbZ2TQNyJbT3/YNLiqTECD0MYeLmAlbiI\r\n" +
    ...     b"tU4hdb6T57ztxy6DL5nk/mfrcO+k4Up+flpGVjm9juWY3jGgszClCLJW0vUH4ToI\r\n" +
    ...     b"1Cb8ew5c7b0f4oYl9AQgySTN1slO64beedMpakS79Mcv5WFwen0vPBQilX7hEYVC\r\n" +
    ...     b"DQnndXm8zU6/MhpVjfoLHd9Tzr0YYQ==\r\n" +
    ...     b"=Equk\r\n" +
    ...     b"-----END PGP SIGNATURE-----\r\n"
    ... )
    >>> pre_sig_text = (
    ...     'tree deadbeef\n' +
    ...     'parent deadc0de\n' +
    ...     'parent baadf00d\n' +
    ...     'author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     'committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700'
    ... )
    >>> desc = " HI! \n   another line with leading spaces\n\nsecond line\n\n\n"
    >>> normalized_desc = stripdesc(desc)
    >>> gpgkeyid = "B577AA76BAE505B1"
    >>> signedcommit = _signedgitcommittext(sig_bytes, pre_sig_text, normalized_desc, gpgkeyid)
    >>> signedcommit == (
    ...     b'tree deadbeef\n' +
    ...     b'parent deadc0de\n' +
    ...     b'parent baadf00d\n' +
    ...     b'author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700\n' +
    ...     b'gpgsig -----BEGIN PGP SIGNATURE-----\n' +
    ...     b' \n' +
    ...     b' iQEzBAABCAAdFiEEurYkrcQEDEhXMjb8tXeqdrrlBbEFAmOPpRIACgkQtXeqdrrl\n' +
    ...     b' BbE8hAf/eybgd1jrovZhs8X/SU2UO4rQnekz5D1BpAVjKUIDTfvuVg7sczTyuXvE\n' +
    ...     b' pkuhkeZd2Is0HvSzWa9dD88VECrwQfHjOFe2Ffb7QdVN4811pZ4+lcGcWKKVG9Oq\n' +
    ...     b' uAtXJgXpBf58Vp9x7wgnbqPFlSUTk5vlbZ2TQNyJbT3/YNLiqTECD0MYeLmAlbiI\n' +
    ...     b' tU4hdb6T57ztxy6DL5nk/mfrcO+k4Up+flpGVjm9juWY3jGgszClCLJW0vUH4ToI\n' +
    ...     b' 1Cb8ew5c7b0f4oYl9AQgySTN1slO64beedMpakS79Mcv5WFwen0vPBQilX7hEYVC\n' +
    ...     b' DQnndXm8zU6/MhpVjfoLHd9Tzr0YYQ==\n' +
    ...     b' =Equk\n' +
    ...     b' -----END PGP SIGNATURE-----\n'
    ...     b'\n' +
    ...     b' HI!\n' +
    ...     b'   another line with leading spaces\n' +
    ...     b'\n' +
    ...     b'second line\n'
    ... )
    True
    """
    sig = sig_bytes.decode("ascii")
    if not sig.endswith("\n"):
        raise error.Abort(
            _("expected signature to end with a newline but was %s") % sig_bytes
        )

    # Remove any carriage returns in case `gpg` was used on Windows:
    # https://github.com/git/git/blob/2e71cbbddd64695d43383c25c7a054ac4ff86882/gpg-interface.c#L985
    sig = sig.replace("\r\n", "\n")

    # The signature returned by `gpg` contains '\n\n' after '-----BEGIN PGP SIGNATURE-----',
    # which is a problem because '\n\n' is used to delimit the start of the commit message
    # in a Git commit object. As a workaround, Git inserts a space after every '\n'
    # (except the last one) as shown here:
    # https://github.com/git/git/blob/2e71cbbddd64695d43383c25c7a054ac4ff86882/commit.c#L1059-L1072
    sig = sig[:-1].replace("\n", "\n ") + "\n"

    signed_text = f"""\
{pre_sig_text}
gpgsig {sig}
{normalized_desc}
"""
    return encodeutf8(signed_text, errors="surrogateescape")

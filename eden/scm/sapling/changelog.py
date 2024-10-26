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
import textwrap
from typing import Dict, List, Optional

import bindings

from . import encoding, error, revlog, util
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
class _changelogrevision:
    # Extensions might modify _defaultextra, so let the constructor below pass
    # it in
    extra = attr.ib()
    manifest = attr.ib(default=nullid)
    user = attr.ib(default="")
    date = attr.ib(default=(0, 0))
    files = attr.ib(default=attr.Factory(list))
    description = attr.ib(default="")


class changelogrevision:
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


class changelogrevision2:
    """Commit fields parser backed by Rust. Supports Git and Hg formats."""

    __slots__ = ("_inner",)

    def __init__(self, text, format):
        if not text:
            text = ""
        elif isinstance(text, bytes):
            text = text.decode(errors="ignore")
        self._inner = bindings.formatutil.CommitFields.from_text(text, format)

    @property
    def manifest(self):
        return self._inner.root_tree()

    @property
    def user(self):
        return self._inner.author_name()

    @property
    def date(self):
        timestamp, tz = self._inner.author_date()
        # for compatibility, use 'float' for timestamp
        return float(timestamp), tz

    @property
    def extra(self):
        extra = self._inner.extras()
        if "branch" not in extra:
            # Is this needed?
            extra = {"branch": "default", **extra}
        # For compatibility, also provide committer in one field
        if "committer" not in extra:
            committer_name = self._inner.committer_name()
            committer_date = self._inner.committer_date()
            if committer_name and committer_date:
                extra["committer"] = committer_name
                extra["committer_date"] = "%d %d" % committer_date
        return extra

    @property
    def files(self):
        files = self._inner.files()
        if files is None:
            return None
        return tuple(files)

    @property
    def description(self):
        return self._inner.description()


def hgcommittext(manifest, files, desc, user, date, extra, use_rust=True):
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
        timestamp, tz = util.parsedate(date)
    else:
        timestamp, tz = util.makedate()

    if extra:
        branch = extra.get("branch")
        if branch in ("default", ""):
            del extra["branch"]
        elif branch in (".", "null", "tip"):
            raise error.RevlogError(_("the name '%s' is reserved") % branch)

    if use_rust:
        # see Rust format-util HgCommitFields for available fields
        fields = {
            "tree": manifest,
            "author": user,
            "date": (int(timestamp), tz),
            "extras": extra,
            "files": sorted(files),
            "message": desc,
        }
        text = bindings.formatutil.hg_commit_fields_to_text(fields).encode()
    else:
        parseddate = "%d %d" % (timestamp, tz)
        if extra:
            extra = encodeextra(extra)
            parseddate = "%s %s" % (parseddate, extra)
        l = [hex(manifest), user, parseddate] + sorted(files) + ["", desc]
        text = encodeutf8("\n".join(l), errors="surrogateescape")
    return text


def gitcommittext(
    tree: bytes,
    parents: List[bytes],
    desc: str,
    user: str,
    date: Optional[str],
    extra: Optional[Dict[str, str]],
    gpgkeyid: Optional[str] = None,
) -> bytes:
    r"""construct raw text (bytes) used by git commit

    If a gpgkeyid is specified, `gpg` will use it to create a signature for
    the unsigned commit object. This signature will be included in the commit
    text exactly as it would in Git.

    Note that while Git supports multiple signature formats (openpgp, x509, ssh),
    Sapling only supports openpgp today.

    >>> tree = b'0' * 20
    >>> desc = " HI! \n   another line with leading spaces\n\nsecond line\n\n\n"
    >>> user = "Alyssa P. Hacker <alyssa@example.com>"
    >>> date = "2000-01-01T00:00:00 +0700"
    >>> print(gitcommittext(tree, [], desc, user, date, None).decode())
    tree 3030303030303030303030303030303030303030
    author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    <BLANKLINE>
     HI!
       another line with leading spaces
    <BLANKLINE>
    second line
    <BLANKLINE>

    >>> p1 = b'2' * 20
    >>> print(gitcommittext(tree, [p1], desc, user, date, None).decode())
    tree 3030303030303030303030303030303030303030
    parent 3232323232323232323232323232323232323232
    author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    <BLANKLINE>
     HI!
       another line with leading spaces
    <BLANKLINE>
    second line
    <BLANKLINE>

    >>> p2 = b'1' * 20
    >>> print(gitcommittext(tree, [p1, p2], desc, user, date, None).decode())
    tree 3030303030303030303030303030303030303030
    parent 3232323232323232323232323232323232323232
    parent 3131313131313131313131313131313131313131
    author Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    committer Alyssa P. Hacker <alyssa@example.com> 946659600 +0700
    <BLANKLINE>
     HI!
       another line with leading spaces
    <BLANKLINE>
    second line
    <BLANKLINE>

    """
    # Example:
    # tree 97e8739f1945a4ba78c9bc1c670718c5dc5c08eb
    # parent 402aab067c4f60fa8ed4868e76b54064fa06a245
    # author svcscm svcscm <svcscm@fb.com> 1626293346 -0700
    # committer Facebook GitHub Bot <facebook-github-bot@users.noreply.github.com> 1626293437 -0700
    #
    # Updating submodules
    committer = (extra.get("committer") if extra else None) or user

    get_date = lambda name: util.parsedate((extra.get(name) if extra else None) or date)

    authordate = get_date("author_date")
    committerdate = get_date("committer_date")

    # date, if not modified, should match either committerdate (usually) or
    # authordate (tests). If modified, the intention is to change authordate,
    # since the committerdate should not never be set manually.
    # This means that `--date` might not be able to update the displayed
    # `{date}` in a Git repo. But it does do something...
    if date != authordate and date != committerdate:
        authordate = date

    # see Rust format-util GitCommitFields for available fields
    fields = {
        "tree": tree,
        "parents": parents,
        "author": user,
        "date": util.parsedate(authordate),
        "committer": committer,
        "committer_date": util.parsedate(committerdate),
        "message": desc,
        "extras": extra or {},
    }
    to_text = bindings.formatutil.git_commit_fields_to_text
    text = to_text(fields).encode()

    if not gpgkeyid:
        return text

    try:
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
                "--always-trust",
                "--yes",
                "--local-user",
                gpgkeyid,
            ],
            stderr=subprocess.PIPE,
            input=text,
        )
    except subprocess.CalledProcessError as ex:
        indented_stderr = textwrap.indent(ex.stderr.decode(errors="ignore"), "  ")
        raise error.Abort(
            _("error when running gpg with gpgkeyid %s:\n%s")
            % (gpgkeyid, indented_stderr)
        )

    gpgsig = sig_bytes.replace(b"\r", b"").decode()
    fields["extras"]["gpgsig"] = gpgsig
    text = to_text(fields).encode()
    return text

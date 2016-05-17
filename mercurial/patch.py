# patch.py - patch file parsing routines
#
# Copyright 2006 Brendan Cully <brendan@kublai.com>
# Copyright 2007 Chris Mason <chris.mason@oracle.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import copy
import email
import errno
import os
import posixpath
import re
import shutil
import tempfile
import zlib

from .i18n import _
from .node import (
    hex,
    short,
)
from . import (
    base85,
    copies,
    diffhelpers,
    encoding,
    error,
    mail,
    mdiff,
    pathutil,
    scmutil,
    util,
)
stringio = util.stringio

gitre = re.compile('diff --git a/(.*) b/(.*)')
tabsplitter = re.compile(r'(\t+|[^\t]+)')

class PatchError(Exception):
    pass


# public functions

def split(stream):
    '''return an iterator of individual patches from a stream'''
    def isheader(line, inheader):
        if inheader and line[0] in (' ', '\t'):
            # continuation
            return True
        if line[0] in (' ', '-', '+'):
            # diff line - don't check for header pattern in there
            return False
        l = line.split(': ', 1)
        return len(l) == 2 and ' ' not in l[0]

    def chunk(lines):
        return stringio(''.join(lines))

    def hgsplit(stream, cur):
        inheader = True

        for line in stream:
            if not line.strip():
                inheader = False
            if not inheader and line.startswith('# HG changeset patch'):
                yield chunk(cur)
                cur = []
                inheader = True

            cur.append(line)

        if cur:
            yield chunk(cur)

    def mboxsplit(stream, cur):
        for line in stream:
            if line.startswith('From '):
                for c in split(chunk(cur[1:])):
                    yield c
                cur = []

            cur.append(line)

        if cur:
            for c in split(chunk(cur[1:])):
                yield c

    def mimesplit(stream, cur):
        def msgfp(m):
            fp = stringio()
            g = email.Generator.Generator(fp, mangle_from_=False)
            g.flatten(m)
            fp.seek(0)
            return fp

        for line in stream:
            cur.append(line)
        c = chunk(cur)

        m = email.Parser.Parser().parse(c)
        if not m.is_multipart():
            yield msgfp(m)
        else:
            ok_types = ('text/plain', 'text/x-diff', 'text/x-patch')
            for part in m.walk():
                ct = part.get_content_type()
                if ct not in ok_types:
                    continue
                yield msgfp(part)

    def headersplit(stream, cur):
        inheader = False

        for line in stream:
            if not inheader and isheader(line, inheader):
                yield chunk(cur)
                cur = []
                inheader = True
            if inheader and not isheader(line, inheader):
                inheader = False

            cur.append(line)

        if cur:
            yield chunk(cur)

    def remainder(cur):
        yield chunk(cur)

    class fiter(object):
        def __init__(self, fp):
            self.fp = fp

        def __iter__(self):
            return self

        def next(self):
            l = self.fp.readline()
            if not l:
                raise StopIteration
            return l

    inheader = False
    cur = []

    mimeheaders = ['content-type']

    if not util.safehasattr(stream, 'next'):
        # http responses, for example, have readline but not next
        stream = fiter(stream)

    for line in stream:
        cur.append(line)
        if line.startswith('# HG changeset patch'):
            return hgsplit(stream, cur)
        elif line.startswith('From '):
            return mboxsplit(stream, cur)
        elif isheader(line, inheader):
            inheader = True
            if line.split(':', 1)[0].lower() in mimeheaders:
                # let email parser handle this
                return mimesplit(stream, cur)
        elif line.startswith('--- ') and inheader:
            # No evil headers seen by diff start, split by hand
            return headersplit(stream, cur)
        # Not enough info, keep reading

    # if we are here, we have a very plain patch
    return remainder(cur)

## Some facility for extensible patch parsing:
# list of pairs ("header to match", "data key")
patchheadermap = [('Date', 'date'),
                  ('Branch', 'branch'),
                  ('Node ID', 'nodeid'),
                 ]

def extract(ui, fileobj):
    '''extract patch from data read from fileobj.

    patch can be a normal patch or contained in an email message.

    return a dictionary. Standard keys are:
      - filename,
      - message,
      - user,
      - date,
      - branch,
      - node,
      - p1,
      - p2.
    Any item can be missing from the dictionary. If filename is missing,
    fileobj did not contain a patch. Caller must unlink filename when done.'''

    # attempt to detect the start of a patch
    # (this heuristic is borrowed from quilt)
    diffre = re.compile(r'^(?:Index:[ \t]|diff[ \t]|RCS file: |'
                        r'retrieving revision [0-9]+(\.[0-9]+)*$|'
                        r'---[ \t].*?^\+\+\+[ \t]|'
                        r'\*\*\*[ \t].*?^---[ \t])', re.MULTILINE|re.DOTALL)

    data = {}
    fd, tmpname = tempfile.mkstemp(prefix='hg-patch-')
    tmpfp = os.fdopen(fd, 'w')
    try:
        msg = email.Parser.Parser().parse(fileobj)

        subject = msg['Subject'] and mail.headdecode(msg['Subject'])
        data['user'] = msg['From'] and mail.headdecode(msg['From'])
        if not subject and not data['user']:
            # Not an email, restore parsed headers if any
            subject = '\n'.join(': '.join(h) for h in msg.items()) + '\n'

        # should try to parse msg['Date']
        parents = []

        if subject:
            if subject.startswith('[PATCH'):
                pend = subject.find(']')
                if pend >= 0:
                    subject = subject[pend + 1:].lstrip()
            subject = re.sub(r'\n[ \t]+', ' ', subject)
            ui.debug('Subject: %s\n' % subject)
        if data['user']:
            ui.debug('From: %s\n' % data['user'])
        diffs_seen = 0
        ok_types = ('text/plain', 'text/x-diff', 'text/x-patch')
        message = ''
        for part in msg.walk():
            content_type = part.get_content_type()
            ui.debug('Content-Type: %s\n' % content_type)
            if content_type not in ok_types:
                continue
            payload = part.get_payload(decode=True)
            m = diffre.search(payload)
            if m:
                hgpatch = False
                hgpatchheader = False
                ignoretext = False

                ui.debug('found patch at byte %d\n' % m.start(0))
                diffs_seen += 1
                cfp = stringio()
                for line in payload[:m.start(0)].splitlines():
                    if line.startswith('# HG changeset patch') and not hgpatch:
                        ui.debug('patch generated by hg export\n')
                        hgpatch = True
                        hgpatchheader = True
                        # drop earlier commit message content
                        cfp.seek(0)
                        cfp.truncate()
                        subject = None
                    elif hgpatchheader:
                        if line.startswith('# User '):
                            data['user'] = line[7:]
                            ui.debug('From: %s\n' % data['user'])
                        elif line.startswith("# Parent "):
                            parents.append(line[9:].lstrip())
                        elif line.startswith("# "):
                            for header, key in patchheadermap:
                                prefix = '# %s ' % header
                                if line.startswith(prefix):
                                    data[key] = line[len(prefix):]
                        else:
                            hgpatchheader = False
                    elif line == '---':
                        ignoretext = True
                    if not hgpatchheader and not ignoretext:
                        cfp.write(line)
                        cfp.write('\n')
                message = cfp.getvalue()
                if tmpfp:
                    tmpfp.write(payload)
                    if not payload.endswith('\n'):
                        tmpfp.write('\n')
            elif not diffs_seen and message and content_type == 'text/plain':
                message += '\n' + payload
    except: # re-raises
        tmpfp.close()
        os.unlink(tmpname)
        raise

    if subject and not message.startswith(subject):
        message = '%s\n%s' % (subject, message)
    data['message'] = message
    tmpfp.close()
    if parents:
        data['p1'] = parents.pop(0)
        if parents:
            data['p2'] = parents.pop(0)

    if diffs_seen:
        data['filename'] = tmpname
    else:
        os.unlink(tmpname)
    return data

class patchmeta(object):
    """Patched file metadata

    'op' is the performed operation within ADD, DELETE, RENAME, MODIFY
    or COPY.  'path' is patched file path. 'oldpath' is set to the
    origin file when 'op' is either COPY or RENAME, None otherwise. If
    file mode is changed, 'mode' is a tuple (islink, isexec) where
    'islink' is True if the file is a symlink and 'isexec' is True if
    the file is executable. Otherwise, 'mode' is None.
    """
    def __init__(self, path):
        self.path = path
        self.oldpath = None
        self.mode = None
        self.op = 'MODIFY'
        self.binary = False

    def setmode(self, mode):
        islink = mode & 0o20000
        isexec = mode & 0o100
        self.mode = (islink, isexec)

    def copy(self):
        other = patchmeta(self.path)
        other.oldpath = self.oldpath
        other.mode = self.mode
        other.op = self.op
        other.binary = self.binary
        return other

    def _ispatchinga(self, afile):
        if afile == '/dev/null':
            return self.op == 'ADD'
        return afile == 'a/' + (self.oldpath or self.path)

    def _ispatchingb(self, bfile):
        if bfile == '/dev/null':
            return self.op == 'DELETE'
        return bfile == 'b/' + self.path

    def ispatching(self, afile, bfile):
        return self._ispatchinga(afile) and self._ispatchingb(bfile)

    def __repr__(self):
        return "<patchmeta %s %r>" % (self.op, self.path)

def readgitpatch(lr):
    """extract git-style metadata about patches from <patchname>"""

    # Filter patch for git information
    gp = None
    gitpatches = []
    for line in lr:
        line = line.rstrip(' \r\n')
        if line.startswith('diff --git a/'):
            m = gitre.match(line)
            if m:
                if gp:
                    gitpatches.append(gp)
                dst = m.group(2)
                gp = patchmeta(dst)
        elif gp:
            if line.startswith('--- '):
                gitpatches.append(gp)
                gp = None
                continue
            if line.startswith('rename from '):
                gp.op = 'RENAME'
                gp.oldpath = line[12:]
            elif line.startswith('rename to '):
                gp.path = line[10:]
            elif line.startswith('copy from '):
                gp.op = 'COPY'
                gp.oldpath = line[10:]
            elif line.startswith('copy to '):
                gp.path = line[8:]
            elif line.startswith('deleted file'):
                gp.op = 'DELETE'
            elif line.startswith('new file mode '):
                gp.op = 'ADD'
                gp.setmode(int(line[-6:], 8))
            elif line.startswith('new mode '):
                gp.setmode(int(line[-6:], 8))
            elif line.startswith('GIT binary patch'):
                gp.binary = True
    if gp:
        gitpatches.append(gp)

    return gitpatches

class linereader(object):
    # simple class to allow pushing lines back into the input stream
    def __init__(self, fp):
        self.fp = fp
        self.buf = []

    def push(self, line):
        if line is not None:
            self.buf.append(line)

    def readline(self):
        if self.buf:
            l = self.buf[0]
            del self.buf[0]
            return l
        return self.fp.readline()

    def __iter__(self):
        while True:
            l = self.readline()
            if not l:
                break
            yield l

class abstractbackend(object):
    def __init__(self, ui):
        self.ui = ui

    def getfile(self, fname):
        """Return target file data and flags as a (data, (islink,
        isexec)) tuple. Data is None if file is missing/deleted.
        """
        raise NotImplementedError

    def setfile(self, fname, data, mode, copysource):
        """Write data to target file fname and set its mode. mode is a
        (islink, isexec) tuple. If data is None, the file content should
        be left unchanged. If the file is modified after being copied,
        copysource is set to the original file name.
        """
        raise NotImplementedError

    def unlink(self, fname):
        """Unlink target file."""
        raise NotImplementedError

    def writerej(self, fname, failed, total, lines):
        """Write rejected lines for fname. total is the number of hunks
        which failed to apply and total the total number of hunks for this
        files.
        """
        pass

    def exists(self, fname):
        raise NotImplementedError

class fsbackend(abstractbackend):
    def __init__(self, ui, basedir):
        super(fsbackend, self).__init__(ui)
        self.opener = scmutil.opener(basedir)

    def _join(self, f):
        return os.path.join(self.opener.base, f)

    def getfile(self, fname):
        if self.opener.islink(fname):
            return (self.opener.readlink(fname), (True, False))

        isexec = False
        try:
            isexec = self.opener.lstat(fname).st_mode & 0o100 != 0
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise
        try:
            return (self.opener.read(fname), (False, isexec))
        except IOError as e:
            if e.errno != errno.ENOENT:
                raise
            return None, None

    def setfile(self, fname, data, mode, copysource):
        islink, isexec = mode
        if data is None:
            self.opener.setflags(fname, islink, isexec)
            return
        if islink:
            self.opener.symlink(data, fname)
        else:
            self.opener.write(fname, data)
            if isexec:
                self.opener.setflags(fname, False, True)

    def unlink(self, fname):
        self.opener.unlinkpath(fname, ignoremissing=True)

    def writerej(self, fname, failed, total, lines):
        fname = fname + ".rej"
        self.ui.warn(
            _("%d out of %d hunks FAILED -- saving rejects to file %s\n") %
            (failed, total, fname))
        fp = self.opener(fname, 'w')
        fp.writelines(lines)
        fp.close()

    def exists(self, fname):
        return self.opener.lexists(fname)

class workingbackend(fsbackend):
    def __init__(self, ui, repo, similarity):
        super(workingbackend, self).__init__(ui, repo.root)
        self.repo = repo
        self.similarity = similarity
        self.removed = set()
        self.changed = set()
        self.copied = []

    def _checkknown(self, fname):
        if self.repo.dirstate[fname] == '?' and self.exists(fname):
            raise PatchError(_('cannot patch %s: file is not tracked') % fname)

    def setfile(self, fname, data, mode, copysource):
        self._checkknown(fname)
        super(workingbackend, self).setfile(fname, data, mode, copysource)
        if copysource is not None:
            self.copied.append((copysource, fname))
        self.changed.add(fname)

    def unlink(self, fname):
        self._checkknown(fname)
        super(workingbackend, self).unlink(fname)
        self.removed.add(fname)
        self.changed.add(fname)

    def close(self):
        wctx = self.repo[None]
        changed = set(self.changed)
        for src, dst in self.copied:
            scmutil.dirstatecopy(self.ui, self.repo, wctx, src, dst)
        if self.removed:
            wctx.forget(sorted(self.removed))
            for f in self.removed:
                if f not in self.repo.dirstate:
                    # File was deleted and no longer belongs to the
                    # dirstate, it was probably marked added then
                    # deleted, and should not be considered by
                    # marktouched().
                    changed.discard(f)
        if changed:
            scmutil.marktouched(self.repo, changed, self.similarity)
        return sorted(self.changed)

class filestore(object):
    def __init__(self, maxsize=None):
        self.opener = None
        self.files = {}
        self.created = 0
        self.maxsize = maxsize
        if self.maxsize is None:
            self.maxsize = 4*(2**20)
        self.size = 0
        self.data = {}

    def setfile(self, fname, data, mode, copied=None):
        if self.maxsize < 0 or (len(data) + self.size) <= self.maxsize:
            self.data[fname] = (data, mode, copied)
            self.size += len(data)
        else:
            if self.opener is None:
                root = tempfile.mkdtemp(prefix='hg-patch-')
                self.opener = scmutil.opener(root)
            # Avoid filename issues with these simple names
            fn = str(self.created)
            self.opener.write(fn, data)
            self.created += 1
            self.files[fname] = (fn, mode, copied)

    def getfile(self, fname):
        if fname in self.data:
            return self.data[fname]
        if not self.opener or fname not in self.files:
            return None, None, None
        fn, mode, copied = self.files[fname]
        return self.opener.read(fn), mode, copied

    def close(self):
        if self.opener:
            shutil.rmtree(self.opener.base)

class repobackend(abstractbackend):
    def __init__(self, ui, repo, ctx, store):
        super(repobackend, self).__init__(ui)
        self.repo = repo
        self.ctx = ctx
        self.store = store
        self.changed = set()
        self.removed = set()
        self.copied = {}

    def _checkknown(self, fname):
        if fname not in self.ctx:
            raise PatchError(_('cannot patch %s: file is not tracked') % fname)

    def getfile(self, fname):
        try:
            fctx = self.ctx[fname]
        except error.LookupError:
            return None, None
        flags = fctx.flags()
        return fctx.data(), ('l' in flags, 'x' in flags)

    def setfile(self, fname, data, mode, copysource):
        if copysource:
            self._checkknown(copysource)
        if data is None:
            data = self.ctx[fname].data()
        self.store.setfile(fname, data, mode, copysource)
        self.changed.add(fname)
        if copysource:
            self.copied[fname] = copysource

    def unlink(self, fname):
        self._checkknown(fname)
        self.removed.add(fname)

    def exists(self, fname):
        return fname in self.ctx

    def close(self):
        return self.changed | self.removed

# @@ -start,len +start,len @@ or @@ -start +start @@ if len is 1
unidesc = re.compile('@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@')
contextdesc = re.compile('(?:---|\*\*\*) (\d+)(?:,(\d+))? (?:---|\*\*\*)')
eolmodes = ['strict', 'crlf', 'lf', 'auto']

class patchfile(object):
    def __init__(self, ui, gp, backend, store, eolmode='strict'):
        self.fname = gp.path
        self.eolmode = eolmode
        self.eol = None
        self.backend = backend
        self.ui = ui
        self.lines = []
        self.exists = False
        self.missing = True
        self.mode = gp.mode
        self.copysource = gp.oldpath
        self.create = gp.op in ('ADD', 'COPY', 'RENAME')
        self.remove = gp.op == 'DELETE'
        if self.copysource is None:
            data, mode = backend.getfile(self.fname)
        else:
            data, mode = store.getfile(self.copysource)[:2]
        if data is not None:
            self.exists = self.copysource is None or backend.exists(self.fname)
            self.missing = False
            if data:
                self.lines = mdiff.splitnewlines(data)
            if self.mode is None:
                self.mode = mode
            if self.lines:
                # Normalize line endings
                if self.lines[0].endswith('\r\n'):
                    self.eol = '\r\n'
                elif self.lines[0].endswith('\n'):
                    self.eol = '\n'
                if eolmode != 'strict':
                    nlines = []
                    for l in self.lines:
                        if l.endswith('\r\n'):
                            l = l[:-2] + '\n'
                        nlines.append(l)
                    self.lines = nlines
        else:
            if self.create:
                self.missing = False
            if self.mode is None:
                self.mode = (False, False)
        if self.missing:
            self.ui.warn(_("unable to find '%s' for patching\n") % self.fname)

        self.hash = {}
        self.dirty = 0
        self.offset = 0
        self.skew = 0
        self.rej = []
        self.fileprinted = False
        self.printfile(False)
        self.hunks = 0

    def writelines(self, fname, lines, mode):
        if self.eolmode == 'auto':
            eol = self.eol
        elif self.eolmode == 'crlf':
            eol = '\r\n'
        else:
            eol = '\n'

        if self.eolmode != 'strict' and eol and eol != '\n':
            rawlines = []
            for l in lines:
                if l and l[-1] == '\n':
                    l = l[:-1] + eol
                rawlines.append(l)
            lines = rawlines

        self.backend.setfile(fname, ''.join(lines), mode, self.copysource)

    def printfile(self, warn):
        if self.fileprinted:
            return
        if warn or self.ui.verbose:
            self.fileprinted = True
        s = _("patching file %s\n") % self.fname
        if warn:
            self.ui.warn(s)
        else:
            self.ui.note(s)


    def findlines(self, l, linenum):
        # looks through the hash and finds candidate lines.  The
        # result is a list of line numbers sorted based on distance
        # from linenum

        cand = self.hash.get(l, [])
        if len(cand) > 1:
            # resort our list of potentials forward then back.
            cand.sort(key=lambda x: abs(x - linenum))
        return cand

    def write_rej(self):
        # our rejects are a little different from patch(1).  This always
        # creates rejects in the same form as the original patch.  A file
        # header is inserted so that you can run the reject through patch again
        # without having to type the filename.
        if not self.rej:
            return
        base = os.path.basename(self.fname)
        lines = ["--- %s\n+++ %s\n" % (base, base)]
        for x in self.rej:
            for l in x.hunk:
                lines.append(l)
                if l[-1] != '\n':
                    lines.append("\n\ No newline at end of file\n")
        self.backend.writerej(self.fname, len(self.rej), self.hunks, lines)

    def apply(self, h):
        if not h.complete():
            raise PatchError(_("bad hunk #%d %s (%d %d %d %d)") %
                            (h.number, h.desc, len(h.a), h.lena, len(h.b),
                            h.lenb))

        self.hunks += 1

        if self.missing:
            self.rej.append(h)
            return -1

        if self.exists and self.create:
            if self.copysource:
                self.ui.warn(_("cannot create %s: destination already "
                               "exists\n") % self.fname)
            else:
                self.ui.warn(_("file %s already exists\n") % self.fname)
            self.rej.append(h)
            return -1

        if isinstance(h, binhunk):
            if self.remove:
                self.backend.unlink(self.fname)
            else:
                l = h.new(self.lines)
                self.lines[:] = l
                self.offset += len(l)
                self.dirty = True
            return 0

        horig = h
        if (self.eolmode in ('crlf', 'lf')
            or self.eolmode == 'auto' and self.eol):
            # If new eols are going to be normalized, then normalize
            # hunk data before patching. Otherwise, preserve input
            # line-endings.
            h = h.getnormalized()

        # fast case first, no offsets, no fuzz
        old, oldstart, new, newstart = h.fuzzit(0, False)
        oldstart += self.offset
        orig_start = oldstart
        # if there's skew we want to emit the "(offset %d lines)" even
        # when the hunk cleanly applies at start + skew, so skip the
        # fast case code
        if (self.skew == 0 and
            diffhelpers.testhunk(old, self.lines, oldstart) == 0):
            if self.remove:
                self.backend.unlink(self.fname)
            else:
                self.lines[oldstart:oldstart + len(old)] = new
                self.offset += len(new) - len(old)
                self.dirty = True
            return 0

        # ok, we couldn't match the hunk. Lets look for offsets and fuzz it
        self.hash = {}
        for x, s in enumerate(self.lines):
            self.hash.setdefault(s, []).append(x)

        for fuzzlen in xrange(self.ui.configint("patch", "fuzz", 2) + 1):
            for toponly in [True, False]:
                old, oldstart, new, newstart = h.fuzzit(fuzzlen, toponly)
                oldstart = oldstart + self.offset + self.skew
                oldstart = min(oldstart, len(self.lines))
                if old:
                    cand = self.findlines(old[0][1:], oldstart)
                else:
                    # Only adding lines with no or fuzzed context, just
                    # take the skew in account
                    cand = [oldstart]

                for l in cand:
                    if not old or diffhelpers.testhunk(old, self.lines, l) == 0:
                        self.lines[l : l + len(old)] = new
                        self.offset += len(new) - len(old)
                        self.skew = l - orig_start
                        self.dirty = True
                        offset = l - orig_start - fuzzlen
                        if fuzzlen:
                            msg = _("Hunk #%d succeeded at %d "
                                    "with fuzz %d "
                                    "(offset %d lines).\n")
                            self.printfile(True)
                            self.ui.warn(msg %
                                (h.number, l + 1, fuzzlen, offset))
                        else:
                            msg = _("Hunk #%d succeeded at %d "
                                    "(offset %d lines).\n")
                            self.ui.note(msg % (h.number, l + 1, offset))
                        return fuzzlen
        self.printfile(True)
        self.ui.warn(_("Hunk #%d FAILED at %d\n") % (h.number, orig_start))
        self.rej.append(horig)
        return -1

    def close(self):
        if self.dirty:
            self.writelines(self.fname, self.lines, self.mode)
        self.write_rej()
        return len(self.rej)

class header(object):
    """patch header
    """
    diffgit_re = re.compile('diff --git a/(.*) b/(.*)$')
    diff_re = re.compile('diff -r .* (.*)$')
    allhunks_re = re.compile('(?:index|deleted file) ')
    pretty_re = re.compile('(?:new file|deleted file) ')
    special_re = re.compile('(?:index|deleted|copy|rename) ')
    newfile_re = re.compile('(?:new file)')

    def __init__(self, header):
        self.header = header
        self.hunks = []

    def binary(self):
        return any(h.startswith('index ') for h in self.header)

    def pretty(self, fp):
        for h in self.header:
            if h.startswith('index '):
                fp.write(_('this modifies a binary file (all or nothing)\n'))
                break
            if self.pretty_re.match(h):
                fp.write(h)
                if self.binary():
                    fp.write(_('this is a binary file\n'))
                break
            if h.startswith('---'):
                fp.write(_('%d hunks, %d lines changed\n') %
                         (len(self.hunks),
                          sum([max(h.added, h.removed) for h in self.hunks])))
                break
            fp.write(h)

    def write(self, fp):
        fp.write(''.join(self.header))

    def allhunks(self):
        return any(self.allhunks_re.match(h) for h in self.header)

    def files(self):
        match = self.diffgit_re.match(self.header[0])
        if match:
            fromfile, tofile = match.groups()
            if fromfile == tofile:
                return [fromfile]
            return [fromfile, tofile]
        else:
            return self.diff_re.match(self.header[0]).groups()

    def filename(self):
        return self.files()[-1]

    def __repr__(self):
        return '<header %s>' % (' '.join(map(repr, self.files())))

    def isnewfile(self):
        return any(self.newfile_re.match(h) for h in self.header)

    def special(self):
        # Special files are shown only at the header level and not at the hunk
        # level for example a file that has been deleted is a special file.
        # The user cannot change the content of the operation, in the case of
        # the deleted file he has to take the deletion or not take it, he
        # cannot take some of it.
        # Newly added files are special if they are empty, they are not special
        # if they have some content as we want to be able to change it
        nocontent = len(self.header) == 2
        emptynewfile = self.isnewfile() and nocontent
        return emptynewfile or \
                any(self.special_re.match(h) for h in self.header)

class recordhunk(object):
    """patch hunk

    XXX shouldn't we merge this with the other hunk class?
    """
    maxcontext = 3

    def __init__(self, header, fromline, toline, proc, before, hunk, after):
        def trimcontext(number, lines):
            delta = len(lines) - self.maxcontext
            if False and delta > 0:
                return number + delta, lines[:self.maxcontext]
            return number, lines

        self.header = header
        self.fromline, self.before = trimcontext(fromline, before)
        self.toline, self.after = trimcontext(toline, after)
        self.proc = proc
        self.hunk = hunk
        self.added, self.removed = self.countchanges(self.hunk)

    def __eq__(self, v):
        if not isinstance(v, recordhunk):
            return False

        return ((v.hunk == self.hunk) and
                (v.proc == self.proc) and
                (self.fromline == v.fromline) and
                (self.header.files() == v.header.files()))

    def __hash__(self):
        return hash((tuple(self.hunk),
            tuple(self.header.files()),
            self.fromline,
            self.proc))

    def countchanges(self, hunk):
        """hunk -> (n+,n-)"""
        add = len([h for h in hunk if h[0] == '+'])
        rem = len([h for h in hunk if h[0] == '-'])
        return add, rem

    def write(self, fp):
        delta = len(self.before) + len(self.after)
        if self.after and self.after[-1] == '\\ No newline at end of file\n':
            delta -= 1
        fromlen = delta + self.removed
        tolen = delta + self.added
        fp.write('@@ -%d,%d +%d,%d @@%s\n' %
                 (self.fromline, fromlen, self.toline, tolen,
                  self.proc and (' ' + self.proc)))
        fp.write(''.join(self.before + self.hunk + self.after))

    pretty = write

    def filename(self):
        return self.header.filename()

    def __repr__(self):
        return '<hunk %r@%d>' % (self.filename(), self.fromline)

def filterpatch(ui, headers, operation=None):
    """Interactively filter patch chunks into applied-only chunks"""
    if operation is None:
        operation = _('record')

    def prompt(skipfile, skipall, query, chunk):
        """prompt query, and process base inputs

        - y/n for the rest of file
        - y/n for the rest
        - ? (help)
        - q (quit)

        Return True/False and possibly updated skipfile and skipall.
        """
        newpatches = None
        if skipall is not None:
            return skipall, skipfile, skipall, newpatches
        if skipfile is not None:
            return skipfile, skipfile, skipall, newpatches
        while True:
            resps = _('[Ynesfdaq?]'
                      '$$ &Yes, record this change'
                      '$$ &No, skip this change'
                      '$$ &Edit this change manually'
                      '$$ &Skip remaining changes to this file'
                      '$$ Record remaining changes to this &file'
                      '$$ &Done, skip remaining changes and files'
                      '$$ Record &all changes to all remaining files'
                      '$$ &Quit, recording no changes'
                      '$$ &? (display help)')
            r = ui.promptchoice("%s %s" % (query, resps))
            ui.write("\n")
            if r == 8: # ?
                for c, t in ui.extractchoices(resps)[1]:
                    ui.write('%s - %s\n' % (c, encoding.lower(t)))
                continue
            elif r == 0: # yes
                ret = True
            elif r == 1: # no
                ret = False
            elif r == 2: # Edit patch
                if chunk is None:
                    ui.write(_('cannot edit patch for whole file'))
                    ui.write("\n")
                    continue
                if chunk.header.binary():
                    ui.write(_('cannot edit patch for binary file'))
                    ui.write("\n")
                    continue
                # Patch comment based on the Git one (based on comment at end of
                # https://mercurial-scm.org/wiki/RecordExtension)
                phelp = '---' + _("""
To remove '-' lines, make them ' ' lines (context).
To remove '+' lines, delete them.
Lines starting with # will be removed from the patch.

If the patch applies cleanly, the edited hunk will immediately be
added to the record list. If it does not apply cleanly, a rejects
file will be generated: you can use that when you try again. If
all lines of the hunk are removed, then the edit is aborted and
the hunk is left unchanged.
""")
                (patchfd, patchfn) = tempfile.mkstemp(prefix="hg-editor-",
                        suffix=".diff", text=True)
                ncpatchfp = None
                try:
                    # Write the initial patch
                    f = os.fdopen(patchfd, "w")
                    chunk.header.write(f)
                    chunk.write(f)
                    f.write('\n'.join(['# ' + i for i in phelp.splitlines()]))
                    f.close()
                    # Start the editor and wait for it to complete
                    editor = ui.geteditor()
                    ret = ui.system("%s \"%s\"" % (editor, patchfn),
                                    environ={'HGUSER': ui.username()})
                    if ret != 0:
                        ui.warn(_("editor exited with exit code %d\n") % ret)
                        continue
                    # Remove comment lines
                    patchfp = open(patchfn)
                    ncpatchfp = stringio()
                    for line in patchfp:
                        if not line.startswith('#'):
                            ncpatchfp.write(line)
                    patchfp.close()
                    ncpatchfp.seek(0)
                    newpatches = parsepatch(ncpatchfp)
                finally:
                    os.unlink(patchfn)
                    del ncpatchfp
                # Signal that the chunk shouldn't be applied as-is, but
                # provide the new patch to be used instead.
                ret = False
            elif r == 3: # Skip
                ret = skipfile = False
            elif r == 4: # file (Record remaining)
                ret = skipfile = True
            elif r == 5: # done, skip remaining
                ret = skipall = False
            elif r == 6: # all
                ret = skipall = True
            elif r == 7: # quit
                raise error.Abort(_('user quit'))
            return ret, skipfile, skipall, newpatches

    seen = set()
    applied = {}        # 'filename' -> [] of chunks
    skipfile, skipall = None, None
    pos, total = 1, sum(len(h.hunks) for h in headers)
    for h in headers:
        pos += len(h.hunks)
        skipfile = None
        fixoffset = 0
        hdr = ''.join(h.header)
        if hdr in seen:
            continue
        seen.add(hdr)
        if skipall is None:
            h.pretty(ui)
        msg = (_('examine changes to %s?') %
               _(' and ').join("'%s'" % f for f in h.files()))
        r, skipfile, skipall, np = prompt(skipfile, skipall, msg, None)
        if not r:
            continue
        applied[h.filename()] = [h]
        if h.allhunks():
            applied[h.filename()] += h.hunks
            continue
        for i, chunk in enumerate(h.hunks):
            if skipfile is None and skipall is None:
                chunk.pretty(ui)
            if total == 1:
                msg = _("record this change to '%s'?") % chunk.filename()
            else:
                idx = pos - len(h.hunks) + i
                msg = _("record change %d/%d to '%s'?") % (idx, total,
                                                           chunk.filename())
            r, skipfile, skipall, newpatches = prompt(skipfile,
                    skipall, msg, chunk)
            if r:
                if fixoffset:
                    chunk = copy.copy(chunk)
                    chunk.toline += fixoffset
                applied[chunk.filename()].append(chunk)
            elif newpatches is not None:
                for newpatch in newpatches:
                    for newhunk in newpatch.hunks:
                        if fixoffset:
                            newhunk.toline += fixoffset
                        applied[newhunk.filename()].append(newhunk)
            else:
                fixoffset += chunk.removed - chunk.added
    return (sum([h for h in applied.itervalues()
               if h[0].special() or len(h) > 1], []), {})
class hunk(object):
    def __init__(self, desc, num, lr, context):
        self.number = num
        self.desc = desc
        self.hunk = [desc]
        self.a = []
        self.b = []
        self.starta = self.lena = None
        self.startb = self.lenb = None
        if lr is not None:
            if context:
                self.read_context_hunk(lr)
            else:
                self.read_unified_hunk(lr)

    def getnormalized(self):
        """Return a copy with line endings normalized to LF."""

        def normalize(lines):
            nlines = []
            for line in lines:
                if line.endswith('\r\n'):
                    line = line[:-2] + '\n'
                nlines.append(line)
            return nlines

        # Dummy object, it is rebuilt manually
        nh = hunk(self.desc, self.number, None, None)
        nh.number = self.number
        nh.desc = self.desc
        nh.hunk = self.hunk
        nh.a = normalize(self.a)
        nh.b = normalize(self.b)
        nh.starta = self.starta
        nh.startb = self.startb
        nh.lena = self.lena
        nh.lenb = self.lenb
        return nh

    def read_unified_hunk(self, lr):
        m = unidesc.match(self.desc)
        if not m:
            raise PatchError(_("bad hunk #%d") % self.number)
        self.starta, self.lena, self.startb, self.lenb = m.groups()
        if self.lena is None:
            self.lena = 1
        else:
            self.lena = int(self.lena)
        if self.lenb is None:
            self.lenb = 1
        else:
            self.lenb = int(self.lenb)
        self.starta = int(self.starta)
        self.startb = int(self.startb)
        diffhelpers.addlines(lr, self.hunk, self.lena, self.lenb, self.a,
                             self.b)
        # if we hit eof before finishing out the hunk, the last line will
        # be zero length.  Lets try to fix it up.
        while len(self.hunk[-1]) == 0:
            del self.hunk[-1]
            del self.a[-1]
            del self.b[-1]
            self.lena -= 1
            self.lenb -= 1
        self._fixnewline(lr)

    def read_context_hunk(self, lr):
        self.desc = lr.readline()
        m = contextdesc.match(self.desc)
        if not m:
            raise PatchError(_("bad hunk #%d") % self.number)
        self.starta, aend = m.groups()
        self.starta = int(self.starta)
        if aend is None:
            aend = self.starta
        self.lena = int(aend) - self.starta
        if self.starta:
            self.lena += 1
        for x in xrange(self.lena):
            l = lr.readline()
            if l.startswith('---'):
                # lines addition, old block is empty
                lr.push(l)
                break
            s = l[2:]
            if l.startswith('- ') or l.startswith('! '):
                u = '-' + s
            elif l.startswith('  '):
                u = ' ' + s
            else:
                raise PatchError(_("bad hunk #%d old text line %d") %
                                 (self.number, x))
            self.a.append(u)
            self.hunk.append(u)

        l = lr.readline()
        if l.startswith('\ '):
            s = self.a[-1][:-1]
            self.a[-1] = s
            self.hunk[-1] = s
            l = lr.readline()
        m = contextdesc.match(l)
        if not m:
            raise PatchError(_("bad hunk #%d") % self.number)
        self.startb, bend = m.groups()
        self.startb = int(self.startb)
        if bend is None:
            bend = self.startb
        self.lenb = int(bend) - self.startb
        if self.startb:
            self.lenb += 1
        hunki = 1
        for x in xrange(self.lenb):
            l = lr.readline()
            if l.startswith('\ '):
                # XXX: the only way to hit this is with an invalid line range.
                # The no-eol marker is not counted in the line range, but I
                # guess there are diff(1) out there which behave differently.
                s = self.b[-1][:-1]
                self.b[-1] = s
                self.hunk[hunki - 1] = s
                continue
            if not l:
                # line deletions, new block is empty and we hit EOF
                lr.push(l)
                break
            s = l[2:]
            if l.startswith('+ ') or l.startswith('! '):
                u = '+' + s
            elif l.startswith('  '):
                u = ' ' + s
            elif len(self.b) == 0:
                # line deletions, new block is empty
                lr.push(l)
                break
            else:
                raise PatchError(_("bad hunk #%d old text line %d") %
                                 (self.number, x))
            self.b.append(s)
            while True:
                if hunki >= len(self.hunk):
                    h = ""
                else:
                    h = self.hunk[hunki]
                hunki += 1
                if h == u:
                    break
                elif h.startswith('-'):
                    continue
                else:
                    self.hunk.insert(hunki - 1, u)
                    break

        if not self.a:
            # this happens when lines were only added to the hunk
            for x in self.hunk:
                if x.startswith('-') or x.startswith(' '):
                    self.a.append(x)
        if not self.b:
            # this happens when lines were only deleted from the hunk
            for x in self.hunk:
                if x.startswith('+') or x.startswith(' '):
                    self.b.append(x[1:])
        # @@ -start,len +start,len @@
        self.desc = "@@ -%d,%d +%d,%d @@\n" % (self.starta, self.lena,
                                             self.startb, self.lenb)
        self.hunk[0] = self.desc
        self._fixnewline(lr)

    def _fixnewline(self, lr):
        l = lr.readline()
        if l.startswith('\ '):
            diffhelpers.fix_newline(self.hunk, self.a, self.b)
        else:
            lr.push(l)

    def complete(self):
        return len(self.a) == self.lena and len(self.b) == self.lenb

    def _fuzzit(self, old, new, fuzz, toponly):
        # this removes context lines from the top and bottom of list 'l'.  It
        # checks the hunk to make sure only context lines are removed, and then
        # returns a new shortened list of lines.
        fuzz = min(fuzz, len(old))
        if fuzz:
            top = 0
            bot = 0
            hlen = len(self.hunk)
            for x in xrange(hlen - 1):
                # the hunk starts with the @@ line, so use x+1
                if self.hunk[x + 1][0] == ' ':
                    top += 1
                else:
                    break
            if not toponly:
                for x in xrange(hlen - 1):
                    if self.hunk[hlen - bot - 1][0] == ' ':
                        bot += 1
                    else:
                        break

            bot = min(fuzz, bot)
            top = min(fuzz, top)
            return old[top:len(old) - bot], new[top:len(new) - bot], top
        return old, new, 0

    def fuzzit(self, fuzz, toponly):
        old, new, top = self._fuzzit(self.a, self.b, fuzz, toponly)
        oldstart = self.starta + top
        newstart = self.startb + top
        # zero length hunk ranges already have their start decremented
        if self.lena and oldstart > 0:
            oldstart -= 1
        if self.lenb and newstart > 0:
            newstart -= 1
        return old, oldstart, new, newstart

class binhunk(object):
    'A binary patch file.'
    def __init__(self, lr, fname):
        self.text = None
        self.delta = False
        self.hunk = ['GIT binary patch\n']
        self._fname = fname
        self._read(lr)

    def complete(self):
        return self.text is not None

    def new(self, lines):
        if self.delta:
            return [applybindelta(self.text, ''.join(lines))]
        return [self.text]

    def _read(self, lr):
        def getline(lr, hunk):
            l = lr.readline()
            hunk.append(l)
            return l.rstrip('\r\n')

        size = 0
        while True:
            line = getline(lr, self.hunk)
            if not line:
                raise PatchError(_('could not extract "%s" binary data')
                                 % self._fname)
            if line.startswith('literal '):
                size = int(line[8:].rstrip())
                break
            if line.startswith('delta '):
                size = int(line[6:].rstrip())
                self.delta = True
                break
        dec = []
        line = getline(lr, self.hunk)
        while len(line) > 1:
            l = line[0]
            if l <= 'Z' and l >= 'A':
                l = ord(l) - ord('A') + 1
            else:
                l = ord(l) - ord('a') + 27
            try:
                dec.append(base85.b85decode(line[1:])[:l])
            except ValueError as e:
                raise PatchError(_('could not decode "%s" binary patch: %s')
                                 % (self._fname, str(e)))
            line = getline(lr, self.hunk)
        text = zlib.decompress(''.join(dec))
        if len(text) != size:
            raise PatchError(_('"%s" length is %d bytes, should be %d')
                             % (self._fname, len(text), size))
        self.text = text

def parsefilename(str):
    # --- filename \t|space stuff
    s = str[4:].rstrip('\r\n')
    i = s.find('\t')
    if i < 0:
        i = s.find(' ')
        if i < 0:
            return s
    return s[:i]

def reversehunks(hunks):
    '''reverse the signs in the hunks given as argument

    This function operates on hunks coming out of patch.filterpatch, that is
    a list of the form: [header1, hunk1, hunk2, header2...]. Example usage:

    >>> rawpatch = """diff --git a/folder1/g b/folder1/g
    ... --- a/folder1/g
    ... +++ b/folder1/g
    ... @@ -1,7 +1,7 @@
    ... +firstline
    ...  c
    ...  1
    ...  2
    ... + 3
    ... -4
    ...  5
    ...  d
    ... +lastline"""
    >>> hunks = parsepatch(rawpatch)
    >>> hunkscomingfromfilterpatch = []
    >>> for h in hunks:
    ...     hunkscomingfromfilterpatch.append(h)
    ...     hunkscomingfromfilterpatch.extend(h.hunks)

    >>> reversedhunks = reversehunks(hunkscomingfromfilterpatch)
    >>> from . import util
    >>> fp = util.stringio()
    >>> for c in reversedhunks:
    ...      c.write(fp)
    >>> fp.seek(0)
    >>> reversedpatch = fp.read()
    >>> print reversedpatch
    diff --git a/folder1/g b/folder1/g
    --- a/folder1/g
    +++ b/folder1/g
    @@ -1,4 +1,3 @@
    -firstline
     c
     1
     2
    @@ -1,6 +2,6 @@
     c
     1
     2
    - 3
    +4
     5
     d
    @@ -5,3 +6,2 @@
     5
     d
    -lastline

    '''

    from . import crecord as crecordmod
    newhunks = []
    for c in hunks:
        if isinstance(c, crecordmod.uihunk):
            # curses hunks encapsulate the record hunk in _hunk
            c = c._hunk
        if isinstance(c, recordhunk):
            for j, line in enumerate(c.hunk):
                if line.startswith("-"):
                    c.hunk[j] = "+" + c.hunk[j][1:]
                elif line.startswith("+"):
                    c.hunk[j] = "-" + c.hunk[j][1:]
            c.added, c.removed = c.removed, c.added
        newhunks.append(c)
    return newhunks

def parsepatch(originalchunks):
    """patch -> [] of headers -> [] of hunks """
    class parser(object):
        """patch parsing state machine"""
        def __init__(self):
            self.fromline = 0
            self.toline = 0
            self.proc = ''
            self.header = None
            self.context = []
            self.before = []
            self.hunk = []
            self.headers = []

        def addrange(self, limits):
            fromstart, fromend, tostart, toend, proc = limits
            self.fromline = int(fromstart)
            self.toline = int(tostart)
            self.proc = proc

        def addcontext(self, context):
            if self.hunk:
                h = recordhunk(self.header, self.fromline, self.toline,
                        self.proc, self.before, self.hunk, context)
                self.header.hunks.append(h)
                self.fromline += len(self.before) + h.removed
                self.toline += len(self.before) + h.added
                self.before = []
                self.hunk = []
            self.context = context

        def addhunk(self, hunk):
            if self.context:
                self.before = self.context
                self.context = []
            self.hunk = hunk

        def newfile(self, hdr):
            self.addcontext([])
            h = header(hdr)
            self.headers.append(h)
            self.header = h

        def addother(self, line):
            pass # 'other' lines are ignored

        def finished(self):
            self.addcontext([])
            return self.headers

        transitions = {
            'file': {'context': addcontext,
                     'file': newfile,
                     'hunk': addhunk,
                     'range': addrange},
            'context': {'file': newfile,
                        'hunk': addhunk,
                        'range': addrange,
                        'other': addother},
            'hunk': {'context': addcontext,
                     'file': newfile,
                     'range': addrange},
            'range': {'context': addcontext,
                      'hunk': addhunk},
            'other': {'other': addother},
            }

    p = parser()
    fp = stringio()
    fp.write(''.join(originalchunks))
    fp.seek(0)

    state = 'context'
    for newstate, data in scanpatch(fp):
        try:
            p.transitions[state][newstate](p, data)
        except KeyError:
            raise PatchError('unhandled transition: %s -> %s' %
                                   (state, newstate))
        state = newstate
    del fp
    return p.finished()

def pathtransform(path, strip, prefix):
    '''turn a path from a patch into a path suitable for the repository

    prefix, if not empty, is expected to be normalized with a / at the end.

    Returns (stripped components, path in repository).

    >>> pathtransform('a/b/c', 0, '')
    ('', 'a/b/c')
    >>> pathtransform('   a/b/c   ', 0, '')
    ('', '   a/b/c')
    >>> pathtransform('   a/b/c   ', 2, '')
    ('a/b/', 'c')
    >>> pathtransform('a/b/c', 0, 'd/e/')
    ('', 'd/e/a/b/c')
    >>> pathtransform('   a//b/c   ', 2, 'd/e/')
    ('a//b/', 'd/e/c')
    >>> pathtransform('a/b/c', 3, '')
    Traceback (most recent call last):
    PatchError: unable to strip away 1 of 3 dirs from a/b/c
    '''
    pathlen = len(path)
    i = 0
    if strip == 0:
        return '', prefix + path.rstrip()
    count = strip
    while count > 0:
        i = path.find('/', i)
        if i == -1:
            raise PatchError(_("unable to strip away %d of %d dirs from %s") %
                             (count, strip, path))
        i += 1
        # consume '//' in the path
        while i < pathlen - 1 and path[i] == '/':
            i += 1
        count -= 1
    return path[:i].lstrip(), prefix + path[i:].rstrip()

def makepatchmeta(backend, afile_orig, bfile_orig, hunk, strip, prefix):
    nulla = afile_orig == "/dev/null"
    nullb = bfile_orig == "/dev/null"
    create = nulla and hunk.starta == 0 and hunk.lena == 0
    remove = nullb and hunk.startb == 0 and hunk.lenb == 0
    abase, afile = pathtransform(afile_orig, strip, prefix)
    gooda = not nulla and backend.exists(afile)
    bbase, bfile = pathtransform(bfile_orig, strip, prefix)
    if afile == bfile:
        goodb = gooda
    else:
        goodb = not nullb and backend.exists(bfile)
    missing = not goodb and not gooda and not create

    # some diff programs apparently produce patches where the afile is
    # not /dev/null, but afile starts with bfile
    abasedir = afile[:afile.rfind('/') + 1]
    bbasedir = bfile[:bfile.rfind('/') + 1]
    if (missing and abasedir == bbasedir and afile.startswith(bfile)
        and hunk.starta == 0 and hunk.lena == 0):
        create = True
        missing = False

    # If afile is "a/b/foo" and bfile is "a/b/foo.orig" we assume the
    # diff is between a file and its backup. In this case, the original
    # file should be patched (see original mpatch code).
    isbackup = (abase == bbase and bfile.startswith(afile))
    fname = None
    if not missing:
        if gooda and goodb:
            if isbackup:
                fname = afile
            else:
                fname = bfile
        elif gooda:
            fname = afile

    if not fname:
        if not nullb:
            if isbackup:
                fname = afile
            else:
                fname = bfile
        elif not nulla:
            fname = afile
        else:
            raise PatchError(_("undefined source and destination files"))

    gp = patchmeta(fname)
    if create:
        gp.op = 'ADD'
    elif remove:
        gp.op = 'DELETE'
    return gp

def scanpatch(fp):
    """like patch.iterhunks, but yield different events

    - ('file',    [header_lines + fromfile + tofile])
    - ('context', [context_lines])
    - ('hunk',    [hunk_lines])
    - ('range',   (-start,len, +start,len, proc))
    """
    lines_re = re.compile(r'@@ -(\d+),(\d+) \+(\d+),(\d+) @@\s*(.*)')
    lr = linereader(fp)

    def scanwhile(first, p):
        """scan lr while predicate holds"""
        lines = [first]
        while True:
            line = lr.readline()
            if not line:
                break
            if p(line):
                lines.append(line)
            else:
                lr.push(line)
                break
        return lines

    while True:
        line = lr.readline()
        if not line:
            break
        if line.startswith('diff --git a/') or line.startswith('diff -r '):
            def notheader(line):
                s = line.split(None, 1)
                return not s or s[0] not in ('---', 'diff')
            header = scanwhile(line, notheader)
            fromfile = lr.readline()
            if fromfile.startswith('---'):
                tofile = lr.readline()
                header += [fromfile, tofile]
            else:
                lr.push(fromfile)
            yield 'file', header
        elif line[0] == ' ':
            yield 'context', scanwhile(line, lambda l: l[0] in ' \\')
        elif line[0] in '-+':
            yield 'hunk', scanwhile(line, lambda l: l[0] in '-+\\')
        else:
            m = lines_re.match(line)
            if m:
                yield 'range', m.groups()
            else:
                yield 'other', line

def scangitpatch(lr, firstline):
    """
    Git patches can emit:
    - rename a to b
    - change b
    - copy a to c
    - change c

    We cannot apply this sequence as-is, the renamed 'a' could not be
    found for it would have been renamed already. And we cannot copy
    from 'b' instead because 'b' would have been changed already. So
    we scan the git patch for copy and rename commands so we can
    perform the copies ahead of time.
    """
    pos = 0
    try:
        pos = lr.fp.tell()
        fp = lr.fp
    except IOError:
        fp = stringio(lr.fp.read())
    gitlr = linereader(fp)
    gitlr.push(firstline)
    gitpatches = readgitpatch(gitlr)
    fp.seek(pos)
    return gitpatches

def iterhunks(fp):
    """Read a patch and yield the following events:
    - ("file", afile, bfile, firsthunk): select a new target file.
    - ("hunk", hunk): a new hunk is ready to be applied, follows a
    "file" event.
    - ("git", gitchanges): current diff is in git format, gitchanges
    maps filenames to gitpatch records. Unique event.
    """
    afile = ""
    bfile = ""
    state = None
    hunknum = 0
    emitfile = newfile = False
    gitpatches = None

    # our states
    BFILE = 1
    context = None
    lr = linereader(fp)

    while True:
        x = lr.readline()
        if not x:
            break
        if state == BFILE and (
            (not context and x[0] == '@')
            or (context is not False and x.startswith('***************'))
            or x.startswith('GIT binary patch')):
            gp = None
            if (gitpatches and
                gitpatches[-1].ispatching(afile, bfile)):
                gp = gitpatches.pop()
            if x.startswith('GIT binary patch'):
                h = binhunk(lr, gp.path)
            else:
                if context is None and x.startswith('***************'):
                    context = True
                h = hunk(x, hunknum + 1, lr, context)
            hunknum += 1
            if emitfile:
                emitfile = False
                yield 'file', (afile, bfile, h, gp and gp.copy() or None)
            yield 'hunk', h
        elif x.startswith('diff --git a/'):
            m = gitre.match(x.rstrip(' \r\n'))
            if not m:
                continue
            if gitpatches is None:
                # scan whole input for git metadata
                gitpatches = scangitpatch(lr, x)
                yield 'git', [g.copy() for g in gitpatches
                              if g.op in ('COPY', 'RENAME')]
                gitpatches.reverse()
            afile = 'a/' + m.group(1)
            bfile = 'b/' + m.group(2)
            while gitpatches and not gitpatches[-1].ispatching(afile, bfile):
                gp = gitpatches.pop()
                yield 'file', ('a/' + gp.path, 'b/' + gp.path, None, gp.copy())
            if not gitpatches:
                raise PatchError(_('failed to synchronize metadata for "%s"')
                                 % afile[2:])
            gp = gitpatches[-1]
            newfile = True
        elif x.startswith('---'):
            # check for a unified diff
            l2 = lr.readline()
            if not l2.startswith('+++'):
                lr.push(l2)
                continue
            newfile = True
            context = False
            afile = parsefilename(x)
            bfile = parsefilename(l2)
        elif x.startswith('***'):
            # check for a context diff
            l2 = lr.readline()
            if not l2.startswith('---'):
                lr.push(l2)
                continue
            l3 = lr.readline()
            lr.push(l3)
            if not l3.startswith("***************"):
                lr.push(l2)
                continue
            newfile = True
            context = True
            afile = parsefilename(x)
            bfile = parsefilename(l2)

        if newfile:
            newfile = False
            emitfile = True
            state = BFILE
            hunknum = 0

    while gitpatches:
        gp = gitpatches.pop()
        yield 'file', ('a/' + gp.path, 'b/' + gp.path, None, gp.copy())

def applybindelta(binchunk, data):
    """Apply a binary delta hunk
    The algorithm used is the algorithm from git's patch-delta.c
    """
    def deltahead(binchunk):
        i = 0
        for c in binchunk:
            i += 1
            if not (ord(c) & 0x80):
                return i
        return i
    out = ""
    s = deltahead(binchunk)
    binchunk = binchunk[s:]
    s = deltahead(binchunk)
    binchunk = binchunk[s:]
    i = 0
    while i < len(binchunk):
        cmd = ord(binchunk[i])
        i += 1
        if (cmd & 0x80):
            offset = 0
            size = 0
            if (cmd & 0x01):
                offset = ord(binchunk[i])
                i += 1
            if (cmd & 0x02):
                offset |= ord(binchunk[i]) << 8
                i += 1
            if (cmd & 0x04):
                offset |= ord(binchunk[i]) << 16
                i += 1
            if (cmd & 0x08):
                offset |= ord(binchunk[i]) << 24
                i += 1
            if (cmd & 0x10):
                size = ord(binchunk[i])
                i += 1
            if (cmd & 0x20):
                size |= ord(binchunk[i]) << 8
                i += 1
            if (cmd & 0x40):
                size |= ord(binchunk[i]) << 16
                i += 1
            if size == 0:
                size = 0x10000
            offset_end = offset + size
            out += data[offset:offset_end]
        elif cmd != 0:
            offset_end = i + cmd
            out += binchunk[i:offset_end]
            i += cmd
        else:
            raise PatchError(_('unexpected delta opcode 0'))
    return out

def applydiff(ui, fp, backend, store, strip=1, prefix='', eolmode='strict'):
    """Reads a patch from fp and tries to apply it.

    Returns 0 for a clean patch, -1 if any rejects were found and 1 if
    there was any fuzz.

    If 'eolmode' is 'strict', the patch content and patched file are
    read in binary mode. Otherwise, line endings are ignored when
    patching then normalized according to 'eolmode'.
    """
    return _applydiff(ui, fp, patchfile, backend, store, strip=strip,
                      prefix=prefix, eolmode=eolmode)

def _applydiff(ui, fp, patcher, backend, store, strip=1, prefix='',
               eolmode='strict'):

    if prefix:
        prefix = pathutil.canonpath(backend.repo.root, backend.repo.getcwd(),
                                    prefix)
        if prefix != '':
            prefix += '/'
    def pstrip(p):
        return pathtransform(p, strip - 1, prefix)[1]

    rejects = 0
    err = 0
    current_file = None

    for state, values in iterhunks(fp):
        if state == 'hunk':
            if not current_file:
                continue
            ret = current_file.apply(values)
            if ret > 0:
                err = 1
        elif state == 'file':
            if current_file:
                rejects += current_file.close()
                current_file = None
            afile, bfile, first_hunk, gp = values
            if gp:
                gp.path = pstrip(gp.path)
                if gp.oldpath:
                    gp.oldpath = pstrip(gp.oldpath)
            else:
                gp = makepatchmeta(backend, afile, bfile, first_hunk, strip,
                                   prefix)
            if gp.op == 'RENAME':
                backend.unlink(gp.oldpath)
            if not first_hunk:
                if gp.op == 'DELETE':
                    backend.unlink(gp.path)
                    continue
                data, mode = None, None
                if gp.op in ('RENAME', 'COPY'):
                    data, mode = store.getfile(gp.oldpath)[:2]
                    # FIXME: failing getfile has never been handled here
                    assert data is not None
                if gp.mode:
                    mode = gp.mode
                    if gp.op == 'ADD':
                        # Added files without content have no hunk and
                        # must be created
                        data = ''
                if data or mode:
                    if (gp.op in ('ADD', 'RENAME', 'COPY')
                        and backend.exists(gp.path)):
                        raise PatchError(_("cannot create %s: destination "
                                           "already exists") % gp.path)
                    backend.setfile(gp.path, data, mode, gp.oldpath)
                continue
            try:
                current_file = patcher(ui, gp, backend, store,
                                       eolmode=eolmode)
            except PatchError as inst:
                ui.warn(str(inst) + '\n')
                current_file = None
                rejects += 1
                continue
        elif state == 'git':
            for gp in values:
                path = pstrip(gp.oldpath)
                data, mode = backend.getfile(path)
                if data is None:
                    # The error ignored here will trigger a getfile()
                    # error in a place more appropriate for error
                    # handling, and will not interrupt the patching
                    # process.
                    pass
                else:
                    store.setfile(path, data, mode)
        else:
            raise error.Abort(_('unsupported parser state: %s') % state)

    if current_file:
        rejects += current_file.close()

    if rejects:
        return -1
    return err

def _externalpatch(ui, repo, patcher, patchname, strip, files,
                   similarity):
    """use <patcher> to apply <patchname> to the working directory.
    returns whether patch was applied with fuzz factor."""

    fuzz = False
    args = []
    cwd = repo.root
    if cwd:
        args.append('-d %s' % util.shellquote(cwd))
    fp = util.popen('%s %s -p%d < %s' % (patcher, ' '.join(args), strip,
                                       util.shellquote(patchname)))
    try:
        for line in fp:
            line = line.rstrip()
            ui.note(line + '\n')
            if line.startswith('patching file '):
                pf = util.parsepatchoutput(line)
                printed_file = False
                files.add(pf)
            elif line.find('with fuzz') >= 0:
                fuzz = True
                if not printed_file:
                    ui.warn(pf + '\n')
                    printed_file = True
                ui.warn(line + '\n')
            elif line.find('saving rejects to file') >= 0:
                ui.warn(line + '\n')
            elif line.find('FAILED') >= 0:
                if not printed_file:
                    ui.warn(pf + '\n')
                    printed_file = True
                ui.warn(line + '\n')
    finally:
        if files:
            scmutil.marktouched(repo, files, similarity)
    code = fp.close()
    if code:
        raise PatchError(_("patch command failed: %s") %
                         util.explainexit(code)[0])
    return fuzz

def patchbackend(ui, backend, patchobj, strip, prefix, files=None,
                 eolmode='strict'):
    if files is None:
        files = set()
    if eolmode is None:
        eolmode = ui.config('patch', 'eol', 'strict')
    if eolmode.lower() not in eolmodes:
        raise error.Abort(_('unsupported line endings type: %s') % eolmode)
    eolmode = eolmode.lower()

    store = filestore()
    try:
        fp = open(patchobj, 'rb')
    except TypeError:
        fp = patchobj
    try:
        ret = applydiff(ui, fp, backend, store, strip=strip, prefix=prefix,
                        eolmode=eolmode)
    finally:
        if fp != patchobj:
            fp.close()
        files.update(backend.close())
        store.close()
    if ret < 0:
        raise PatchError(_('patch failed to apply'))
    return ret > 0

def internalpatch(ui, repo, patchobj, strip, prefix='', files=None,
                  eolmode='strict', similarity=0):
    """use builtin patch to apply <patchobj> to the working directory.
    returns whether patch was applied with fuzz factor."""
    backend = workingbackend(ui, repo, similarity)
    return patchbackend(ui, backend, patchobj, strip, prefix, files, eolmode)

def patchrepo(ui, repo, ctx, store, patchobj, strip, prefix, files=None,
              eolmode='strict'):
    backend = repobackend(ui, repo, ctx, store)
    return patchbackend(ui, backend, patchobj, strip, prefix, files, eolmode)

def patch(ui, repo, patchname, strip=1, prefix='', files=None, eolmode='strict',
          similarity=0):
    """Apply <patchname> to the working directory.

    'eolmode' specifies how end of lines should be handled. It can be:
    - 'strict': inputs are read in binary mode, EOLs are preserved
    - 'crlf': EOLs are ignored when patching and reset to CRLF
    - 'lf': EOLs are ignored when patching and reset to LF
    - None: get it from user settings, default to 'strict'
    'eolmode' is ignored when using an external patcher program.

    Returns whether patch was applied with fuzz factor.
    """
    patcher = ui.config('ui', 'patch')
    if files is None:
        files = set()
    if patcher:
        return _externalpatch(ui, repo, patcher, patchname, strip,
                              files, similarity)
    return internalpatch(ui, repo, patchname, strip, prefix, files, eolmode,
                         similarity)

def changedfiles(ui, repo, patchpath, strip=1):
    backend = fsbackend(ui, repo.root)
    with open(patchpath, 'rb') as fp:
        changed = set()
        for state, values in iterhunks(fp):
            if state == 'file':
                afile, bfile, first_hunk, gp = values
                if gp:
                    gp.path = pathtransform(gp.path, strip - 1, '')[1]
                    if gp.oldpath:
                        gp.oldpath = pathtransform(gp.oldpath, strip - 1, '')[1]
                else:
                    gp = makepatchmeta(backend, afile, bfile, first_hunk, strip,
                                       '')
                changed.add(gp.path)
                if gp.op == 'RENAME':
                    changed.add(gp.oldpath)
            elif state not in ('hunk', 'git'):
                raise error.Abort(_('unsupported parser state: %s') % state)
        return changed

class GitDiffRequired(Exception):
    pass

def diffallopts(ui, opts=None, untrusted=False, section='diff'):
    '''return diffopts with all features supported and parsed'''
    return difffeatureopts(ui, opts=opts, untrusted=untrusted, section=section,
                           git=True, whitespace=True, formatchanging=True)

diffopts = diffallopts

def difffeatureopts(ui, opts=None, untrusted=False, section='diff', git=False,
                    whitespace=False, formatchanging=False):
    '''return diffopts with only opted-in features parsed

    Features:
    - git: git-style diffs
    - whitespace: whitespace options like ignoreblanklines and ignorews
    - formatchanging: options that will likely break or cause correctness issues
      with most diff parsers
    '''
    def get(key, name=None, getter=ui.configbool, forceplain=None):
        if opts:
            v = opts.get(key)
            if v:
                return v
        if forceplain is not None and ui.plain():
            return forceplain
        return getter(section, name or key, None, untrusted=untrusted)

    # core options, expected to be understood by every diff parser
    buildopts = {
        'nodates': get('nodates'),
        'showfunc': get('show_function', 'showfunc'),
        'context': get('unified', getter=ui.config),
    }

    if git:
        buildopts['git'] = get('git')
    if whitespace:
        buildopts['ignorews'] = get('ignore_all_space', 'ignorews')
        buildopts['ignorewsamount'] = get('ignore_space_change',
                                          'ignorewsamount')
        buildopts['ignoreblanklines'] = get('ignore_blank_lines',
                                            'ignoreblanklines')
    if formatchanging:
        buildopts['text'] = opts and opts.get('text')
        buildopts['nobinary'] = get('nobinary', forceplain=False)
        buildopts['noprefix'] = get('noprefix', forceplain=False)

    return mdiff.diffopts(**buildopts)

def diff(repo, node1=None, node2=None, match=None, changes=None, opts=None,
         losedatafn=None, prefix='', relroot=''):
    '''yields diff of changes to files between two nodes, or node and
    working directory.

    if node1 is None, use first dirstate parent instead.
    if node2 is None, compare node1 with working directory.

    losedatafn(**kwarg) is a callable run when opts.upgrade=True and
    every time some change cannot be represented with the current
    patch format. Return False to upgrade to git patch format, True to
    accept the loss or raise an exception to abort the diff. It is
    called with the name of current file being diffed as 'fn'. If set
    to None, patches will always be upgraded to git format when
    necessary.

    prefix is a filename prefix that is prepended to all filenames on
    display (used for subrepos).

    relroot, if not empty, must be normalized with a trailing /. Any match
    patterns that fall outside it will be ignored.'''

    if opts is None:
        opts = mdiff.defaultopts

    if not node1 and not node2:
        node1 = repo.dirstate.p1()

    def lrugetfilectx():
        cache = {}
        order = collections.deque()
        def getfilectx(f, ctx):
            fctx = ctx.filectx(f, filelog=cache.get(f))
            if f not in cache:
                if len(cache) > 20:
                    del cache[order.popleft()]
                cache[f] = fctx.filelog()
            else:
                order.remove(f)
            order.append(f)
            return fctx
        return getfilectx
    getfilectx = lrugetfilectx()

    ctx1 = repo[node1]
    ctx2 = repo[node2]

    relfiltered = False
    if relroot != '' and match.always():
        # as a special case, create a new matcher with just the relroot
        pats = [relroot]
        match = scmutil.match(ctx2, pats, default='path')
        relfiltered = True

    if not changes:
        changes = repo.status(ctx1, ctx2, match=match)
    modified, added, removed = changes[:3]

    if not modified and not added and not removed:
        return []

    if repo.ui.debugflag:
        hexfunc = hex
    else:
        hexfunc = short
    revs = [hexfunc(node) for node in [ctx1.node(), ctx2.node()] if node]

    copy = {}
    if opts.git or opts.upgrade:
        copy = copies.pathcopies(ctx1, ctx2, match=match)

    if relroot is not None:
        if not relfiltered:
            # XXX this would ideally be done in the matcher, but that is
            # generally meant to 'or' patterns, not 'and' them. In this case we
            # need to 'and' all the patterns from the matcher with relroot.
            def filterrel(l):
                return [f for f in l if f.startswith(relroot)]
            modified = filterrel(modified)
            added = filterrel(added)
            removed = filterrel(removed)
            relfiltered = True
        # filter out copies where either side isn't inside the relative root
        copy = dict(((dst, src) for (dst, src) in copy.iteritems()
                     if dst.startswith(relroot)
                     and src.startswith(relroot)))

    modifiedset = set(modified)
    addedset = set(added)
    removedset = set(removed)
    for f in modified:
        if f not in ctx1:
            # Fix up added, since merged-in additions appear as
            # modifications during merges
            modifiedset.remove(f)
            addedset.add(f)
    for f in removed:
        if f not in ctx1:
            # Merged-in additions that are then removed are reported as removed.
            # They are not in ctx1, so We don't want to show them in the diff.
            removedset.remove(f)
    modified = sorted(modifiedset)
    added = sorted(addedset)
    removed = sorted(removedset)
    for dst, src in copy.items():
        if src not in ctx1:
            # Files merged in during a merge and then copied/renamed are
            # reported as copies. We want to show them in the diff as additions.
            del copy[dst]

    def difffn(opts, losedata):
        return trydiff(repo, revs, ctx1, ctx2, modified, added, removed,
                       copy, getfilectx, opts, losedata, prefix, relroot)
    if opts.upgrade and not opts.git:
        try:
            def losedata(fn):
                if not losedatafn or not losedatafn(fn=fn):
                    raise GitDiffRequired
            # Buffer the whole output until we are sure it can be generated
            return list(difffn(opts.copy(git=False), losedata))
        except GitDiffRequired:
            return difffn(opts.copy(git=True), None)
    else:
        return difffn(opts, None)

def difflabel(func, *args, **kw):
    '''yields 2-tuples of (output, label) based on the output of func()'''
    headprefixes = [('diff', 'diff.diffline'),
                    ('copy', 'diff.extended'),
                    ('rename', 'diff.extended'),
                    ('old', 'diff.extended'),
                    ('new', 'diff.extended'),
                    ('deleted', 'diff.extended'),
                    ('---', 'diff.file_a'),
                    ('+++', 'diff.file_b')]
    textprefixes = [('@', 'diff.hunk'),
                    ('-', 'diff.deleted'),
                    ('+', 'diff.inserted')]
    head = False
    for chunk in func(*args, **kw):
        lines = chunk.split('\n')
        for i, line in enumerate(lines):
            if i != 0:
                yield ('\n', '')
            if head:
                if line.startswith('@'):
                    head = False
            else:
                if line and line[0] not in ' +-@\\':
                    head = True
            stripline = line
            diffline = False
            if not head and line and line[0] in '+-':
                # highlight tabs and trailing whitespace, but only in
                # changed lines
                stripline = line.rstrip()
                diffline = True

            prefixes = textprefixes
            if head:
                prefixes = headprefixes
            for prefix, label in prefixes:
                if stripline.startswith(prefix):
                    if diffline:
                        for token in tabsplitter.findall(stripline):
                            if '\t' == token[0]:
                                yield (token, 'diff.tab')
                            else:
                                yield (token, label)
                    else:
                        yield (stripline, label)
                    break
            else:
                yield (line, '')
            if line != stripline:
                yield (line[len(stripline):], 'diff.trailingwhitespace')

def diffui(*args, **kw):
    '''like diff(), but yields 2-tuples of (output, label) for ui.write()'''
    return difflabel(diff, *args, **kw)

def _filepairs(modified, added, removed, copy, opts):
    '''generates tuples (f1, f2, copyop), where f1 is the name of the file
    before and f2 is the the name after. For added files, f1 will be None,
    and for removed files, f2 will be None. copyop may be set to None, 'copy'
    or 'rename' (the latter two only if opts.git is set).'''
    gone = set()

    copyto = dict([(v, k) for k, v in copy.items()])

    addedset, removedset = set(added), set(removed)

    for f in sorted(modified + added + removed):
        copyop = None
        f1, f2 = f, f
        if f in addedset:
            f1 = None
            if f in copy:
                if opts.git:
                    f1 = copy[f]
                    if f1 in removedset and f1 not in gone:
                        copyop = 'rename'
                        gone.add(f1)
                    else:
                        copyop = 'copy'
        elif f in removedset:
            f2 = None
            if opts.git:
                # have we already reported a copy above?
                if (f in copyto and copyto[f] in addedset
                    and copy[copyto[f]] == f):
                    continue
        yield f1, f2, copyop

def trydiff(repo, revs, ctx1, ctx2, modified, added, removed,
            copy, getfilectx, opts, losedatafn, prefix, relroot):
    '''given input data, generate a diff and yield it in blocks

    If generating a diff would lose data like flags or binary data and
    losedatafn is not None, it will be called.

    relroot is removed and prefix is added to every path in the diff output.

    If relroot is not empty, this function expects every path in modified,
    added, removed and copy to start with it.'''

    def gitindex(text):
        if not text:
            text = ""
        l = len(text)
        s = util.sha1('blob %d\0' % l)
        s.update(text)
        return s.hexdigest()

    if opts.noprefix:
        aprefix = bprefix = ''
    else:
        aprefix = 'a/'
        bprefix = 'b/'

    def diffline(f, revs):
        revinfo = ' '.join(["-r %s" % rev for rev in revs])
        return 'diff %s %s' % (revinfo, f)

    date1 = util.datestr(ctx1.date())
    date2 = util.datestr(ctx2.date())

    gitmode = {'l': '120000', 'x': '100755', '': '100644'}

    if relroot != '' and (repo.ui.configbool('devel', 'all')
                          or repo.ui.configbool('devel', 'check-relroot')):
        for f in modified + added + removed + copy.keys() + copy.values():
            if f is not None and not f.startswith(relroot):
                raise AssertionError(
                    "file %s doesn't start with relroot %s" % (f, relroot))

    for f1, f2, copyop in _filepairs(modified, added, removed, copy, opts):
        content1 = None
        content2 = None
        flag1 = None
        flag2 = None
        if f1:
            content1 = getfilectx(f1, ctx1).data()
            if opts.git or losedatafn:
                flag1 = ctx1.flags(f1)
        if f2:
            content2 = getfilectx(f2, ctx2).data()
            if opts.git or losedatafn:
                flag2 = ctx2.flags(f2)
        binary = False
        if opts.git or losedatafn:
            binary = util.binary(content1) or util.binary(content2)

        if losedatafn and not opts.git:
            if (binary or
                # copy/rename
                f2 in copy or
                # empty file creation
                (not f1 and not content2) or
                # empty file deletion
                (not content1 and not f2) or
                # create with flags
                (not f1 and flag2) or
                # change flags
                (f1 and f2 and flag1 != flag2)):
                losedatafn(f2 or f1)

        path1 = f1 or f2
        path2 = f2 or f1
        path1 = posixpath.join(prefix, path1[len(relroot):])
        path2 = posixpath.join(prefix, path2[len(relroot):])
        header = []
        if opts.git:
            header.append('diff --git %s%s %s%s' %
                          (aprefix, path1, bprefix, path2))
            if not f1: # added
                header.append('new file mode %s' % gitmode[flag2])
            elif not f2: # removed
                header.append('deleted file mode %s' % gitmode[flag1])
            else:  # modified/copied/renamed
                mode1, mode2 = gitmode[flag1], gitmode[flag2]
                if mode1 != mode2:
                    header.append('old mode %s' % mode1)
                    header.append('new mode %s' % mode2)
                if copyop is not None:
                    header.append('%s from %s' % (copyop, path1))
                    header.append('%s to %s' % (copyop, path2))
        elif revs and not repo.ui.quiet:
            header.append(diffline(path1, revs))

        if binary and opts.git and not opts.nobinary:
            text = mdiff.b85diff(content1, content2)
            if text:
                header.append('index %s..%s' %
                              (gitindex(content1), gitindex(content2)))
        else:
            text = mdiff.unidiff(content1, date1,
                                 content2, date2,
                                 path1, path2, opts=opts)
        if header and (text or len(header) > 1):
            yield '\n'.join(header) + '\n'
        if text:
            yield text

def diffstatsum(stats):
    maxfile, maxtotal, addtotal, removetotal, binary = 0, 0, 0, 0, False
    for f, a, r, b in stats:
        maxfile = max(maxfile, encoding.colwidth(f))
        maxtotal = max(maxtotal, a + r)
        addtotal += a
        removetotal += r
        binary = binary or b

    return maxfile, maxtotal, addtotal, removetotal, binary

def diffstatdata(lines):
    diffre = re.compile('^diff .*-r [a-z0-9]+\s(.*)$')

    results = []
    filename, adds, removes, isbinary = None, 0, 0, False

    def addresult():
        if filename:
            results.append((filename, adds, removes, isbinary))

    for line in lines:
        if line.startswith('diff'):
            addresult()
            # set numbers to 0 anyway when starting new file
            adds, removes, isbinary = 0, 0, False
            if line.startswith('diff --git a/'):
                filename = gitre.search(line).group(2)
            elif line.startswith('diff -r'):
                # format: "diff -r ... -r ... filename"
                filename = diffre.search(line).group(1)
        elif line.startswith('+') and not line.startswith('+++ '):
            adds += 1
        elif line.startswith('-') and not line.startswith('--- '):
            removes += 1
        elif (line.startswith('GIT binary patch') or
              line.startswith('Binary file')):
            isbinary = True
    addresult()
    return results

def diffstat(lines, width=80, git=False):
    output = []
    stats = diffstatdata(lines)
    maxname, maxtotal, totaladds, totalremoves, hasbinary = diffstatsum(stats)

    countwidth = len(str(maxtotal))
    if hasbinary and countwidth < 3:
        countwidth = 3
    graphwidth = width - countwidth - maxname - 6
    if graphwidth < 10:
        graphwidth = 10

    def scale(i):
        if maxtotal <= graphwidth:
            return i
        # If diffstat runs out of room it doesn't print anything,
        # which isn't very useful, so always print at least one + or -
        # if there were at least some changes.
        return max(i * graphwidth // maxtotal, int(bool(i)))

    for filename, adds, removes, isbinary in stats:
        if isbinary:
            count = 'Bin'
        else:
            count = adds + removes
        pluses = '+' * scale(adds)
        minuses = '-' * scale(removes)
        output.append(' %s%s |  %*s %s%s\n' %
                      (filename, ' ' * (maxname - encoding.colwidth(filename)),
                       countwidth, count, pluses, minuses))

    if stats:
        output.append(_(' %d files changed, %d insertions(+), '
                        '%d deletions(-)\n')
                      % (len(stats), totaladds, totalremoves))

    return ''.join(output)

def diffstatui(*args, **kw):
    '''like diffstat(), but yields 2-tuples of (output, label) for
    ui.write()
    '''

    for line in diffstat(*args, **kw).splitlines():
        if line and line[-1] in '+-':
            name, graph = line.rsplit(' ', 1)
            yield (name + ' ', '')
            m = re.search(r'\++', graph)
            if m:
                yield (m.group(0), 'diffstat.inserted')
            m = re.search(r'-+', graph)
            if m:
                yield (m.group(0), 'diffstat.deleted')
        else:
            yield (line, '')
        yield ('\n', '')

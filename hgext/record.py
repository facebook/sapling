# record.py
#
# Copyright 2007 Bryan O'Sullivan <bos@serpentine.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.

'''interactive change selection during commit'''

from mercurial.i18n import _
from mercurial import cmdutil, commands, cmdutil, hg, mdiff, patch, revlog
from mercurial import util
import copy, cStringIO, errno, operator, os, re, shutil, tempfile

lines_re = re.compile(r'@@ -(\d+),(\d+) \+(\d+),(\d+) @@\s*(.*)')

def scanpatch(fp):
    lr = patch.linereader(fp)

    def scanwhile(first, p):
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
        if line.startswith('diff --git a/'):
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
                raise patch.PatchError('unknown patch content: %r' % line)

class header(object):
    diff_re = re.compile('diff --git a/(.*) b/(.*)$')
    allhunks_re = re.compile('(?:index|new file|deleted file) ')
    pretty_re = re.compile('(?:new file|deleted file) ')
    special_re = re.compile('(?:index|new|deleted|copy|rename) ')

    def __init__(self, header):
        self.header = header
        self.hunks = []

    def binary(self):
        for h in self.header:
            if h.startswith('index '):
                return True
        
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
                          sum([h.added + h.removed for h in self.hunks])))
                break
            fp.write(h)

    def write(self, fp):
        fp.write(''.join(self.header))

    def allhunks(self):
        for h in self.header:
            if self.allhunks_re.match(h):
                return True

    def files(self):
        fromfile, tofile = self.diff_re.match(self.header[0]).groups()
        if fromfile == tofile:
            return [fromfile]
        return [fromfile, tofile]

    def filename(self):
        return self.files()[-1]

    def __repr__(self):
        return '<header %s>' % (' '.join(map(repr, self.files())))

    def special(self):
        for h in self.header:
            if self.special_re.match(h):
                return True

def countchanges(hunk):
    add = len([h for h in hunk if h[0] == '+'])
    rem = len([h for h in hunk if h[0] == '-'])
    return add, rem

class hunk(object):
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
        self.added, self.removed = countchanges(self.hunk)

    def write(self, fp):
        delta = len(self.before) + len(self.after)
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

def parsepatch(fp):
    class parser(object):
        def __init__(self):
            self.fromline = 0
            self.toline = 0
            self.proc = ''
            self.header = None
            self.context = []
            self.before = []
            self.hunk = []
            self.stream = []

        def addrange(self, (fromstart, fromend, tostart, toend, proc)):
            self.fromline = int(fromstart)
            self.toline = int(tostart)
            self.proc = proc

        def addcontext(self, context):
            if self.hunk:
                h = hunk(self.header, self.fromline, self.toline, self.proc,
                         self.before, self.hunk, context)
                self.header.hunks.append(h)
                self.stream.append(h)
                self.fromline += len(self.before) + h.removed
                self.toline += len(self.before) + h.added
                self.before = []
                self.hunk = []
                self.proc = ''
            self.context = context

        def addhunk(self, hunk):
            if self.context:
                self.before = self.context
                self.context = []
            self.hunk = data

        def newfile(self, hdr):
            self.addcontext([])
            h = header(hdr)
            self.stream.append(h)
            self.header = h

        def finished(self):
            self.addcontext([])
            return self.stream

        transitions = {
            'file': {'context': addcontext,
                     'file': newfile,
                     'hunk': addhunk,
                     'range': addrange},
            'context': {'file': newfile,
                        'hunk': addhunk,
                        'range': addrange},
            'hunk': {'context': addcontext,
                     'file': newfile,
                     'range': addrange},
            'range': {'context': addcontext,
                      'hunk': addhunk},
            }
             
    p = parser()

    state = 'context'
    for newstate, data in scanpatch(fp):
        try:
            p.transitions[state][newstate](p, data)
        except KeyError:
            raise patch.PatchError('unhandled transition: %s -> %s' %
                                   (state, newstate))
        state = newstate
    return p.finished()

def filterpatch(ui, chunks):
    chunks = list(chunks)
    chunks.reverse()
    seen = {}
    def consumefile():
        consumed = []
        while chunks:
            if isinstance(chunks[-1], header):
                break
            else:
                consumed.append(chunks.pop())
        return consumed
    resp = None
    applied = {}
    while chunks:
        chunk = chunks.pop()
        if isinstance(chunk, header):
            fixoffset = 0
            hdr = ''.join(chunk.header)
            if hdr in seen:
                consumefile()
                continue
            seen[hdr] = True
            if not resp:
                chunk.pretty(ui)
            r = resp or ui.prompt(_('record changes to %s? [y]es [n]o') %
                                  _(' and ').join(map(repr, chunk.files())),
                                  '(?:|[yYnNqQaA])$') or 'y'
            if r in 'aA':
                r = 'y'
                resp = 'y'
            if r in 'qQ':
                raise util.Abort(_('user quit'))
            if r in 'yY':
                applied[chunk.filename()] = [chunk]
                if chunk.allhunks():
                    applied[chunk.filename()] += consumefile()
            else:
                consumefile()
        else:
            if not resp:
                chunk.pretty(ui)
            r = resp or ui.prompt(_('record this change to %r? [y]es [n]o') %
                                  chunk.filename(), '(?:|[yYnNqQaA])$') or 'y'
            if r in 'aA':
                r = 'y'
                resp = 'y'
            if r in 'qQ':
                raise util.Abort(_('user quit'))
            if r in 'yY':
                if fixoffset:
                    chunk = copy.copy(chunk)
                    chunk.toline += fixoffset
                applied[chunk.filename()].append(chunk)
            else:
                fixoffset += chunk.removed - chunk.added
    return reduce(operator.add, [h for h in applied.itervalues()
                                 if h[0].special() or len(h) > 1], [])

def record(ui, repo, *pats, **opts):
    '''interactively select changes to commit'''

    if not ui.interactive:
        raise util.Abort(_('running non-interactively, use commit instead'))

    def recordfunc(ui, repo, files, message, match, opts):
        if files:
            changes = None
        else:
            changes = repo.status(files=files, match=match)[:5]
            modified, added, removed = changes[:3]
            files = modified + added + removed
        diffopts = mdiff.diffopts(git=True, nodates=True)
        fp = cStringIO.StringIO()
        patch.diff(repo, repo.dirstate.parents()[0], files=files,
                   match=match, changes=changes, opts=diffopts, fp=fp)
        fp.seek(0)

        chunks = filterpatch(ui, parsepatch(fp))
        del fp

        contenders = {}
        for h in chunks:
            try: contenders.update(dict.fromkeys(h.files()))
            except AttributeError: pass
            
        newfiles = [f for f in files if f in contenders]

        if not newfiles:
            ui.status(_('no changes to record\n'))
            return 0

        if changes is None:
            changes = repo.status(files=newfiles, match=match)[:5]
        modified = dict.fromkeys(changes[0])

        backups = {}
        backupdir = repo.join('record-backups')
        try:
            os.mkdir(backupdir)
        except OSError, err:
            if err.errno == errno.EEXIST:
                pass
        try:
            for f in newfiles:
                if f not in modified:
                    continue
                fd, tmpname = tempfile.mkstemp(prefix=f.replace('/', '_')+'.',
                                               dir=backupdir)
                os.close(fd)
                ui.debug('backup %r as %r\n' % (f, tmpname))
                util.copyfile(repo.wjoin(f), tmpname)
                backups[f] = tmpname

            fp = cStringIO.StringIO()
            for c in chunks:
                if c.filename() in backups:
                    c.write(fp)
            dopatch = fp.tell()
            fp.seek(0)

            if backups:
                hg.revert(repo, repo.dirstate.parents()[0], backups.has_key)

            if dopatch:
                ui.debug('applying patch\n')
                ui.debug(fp.getvalue())
                patch.internalpatch(fp, ui, 1, repo.root)
            del fp

            repo.commit(newfiles, message, opts['user'], opts['date'], match,
                        force_editor=opts.get('force_editor'))
            return 0
        finally:
            try:
                for realname, tmpname in backups.iteritems():
                    ui.debug('restoring %r to %r\n' % (tmpname, realname))
                    util.copyfile(tmpname, realname)
                    os.unlink(tmpname)
                os.rmdir(backupdir)
            except OSError:
                pass
    return cmdutil.commit(ui, repo, recordfunc, pats, opts)

cmdtable = {
    'record':
    (record, [('A', 'addremove', None,
               _('mark new/missing files as added/removed before committing')),
              ('d', 'date', '', _('record datecode as commit date')),
              ('u', 'user', '', _('record user as commiter')),
              ] + commands.walkopts + commands.commitopts,
     _('hg record [FILE]...')),
    }

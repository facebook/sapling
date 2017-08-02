from mercurial.i18n import _
from mercurial import util

import base64
import contextlib
import fcntl

class filetransaction(object):
    def __init__(self, report, opener):
        self.report = report
        self.opener = opener
        self.closed = False
        self.journalfile = 'filejournals'
        self.map = {}

    @contextlib.contextmanager
    def _lock(self, mode):
        f = self.opener.open(self.journalfile, mode)
        try:
            fcntl.lockf(f, fcntl.LOCK_EX)
            yield f
        finally:
            f.close()

    def add(self, file, offset, data=None):
        # we need to base64 this, so we avoid newlines
        encdata = base64.b64encode(util.pickle.dumps(data))
        with self._lock(mode='a') as f:
            f.seek(0, 2)
            writeoffset = f.tell()
            f.write('%d\0%s\0%s\n' % (offset, file, encdata))
            f.flush()
            self.map[file] = (file, offset, data, writeoffset)

    def close(self):
        self.closed = True

    def _read(self):
        entries = []
        with self._lock(mode='r+') as f:
            for line in f.readlines():
                e = line.split('\0', 2)
                decdata = util.pickle.loads(base64.b64decode(e[2]))
                entries.append((e[1].rstrip(), int(e[0]), decdata))
        return entries

    def abort(self):
        self.report(_('transaction abort!\n'))
        if self.opener.exists(self.journalfile):
            for filename, offset, __ in self._read():
                fp = self.opener(filename, 'a', checkambig=True)
                fp.truncate(offset)
                fp.close()
            self.opener.unlink(self.journalfile)
        self.report(_('rollback complete\n'))

    def release(self):
        if self.closed:
            self.opener.unlink(self.journalfile)
            return
        self.abort()

    def find(self, file):
        if file in self.map:
            filename, offset, data, __ = self.map[file]
            return (filename, offset, data)
        last = None
        # TODO: T19177624 Fix this O(entries) behavior
        for filename, offset, data in self._read():
            if filename == file:
                last = (filename, offset, data)
        return last

    def replace(self, file, offset, data=None):
        self.map[file] = (file, offset, data)
        self.add(file, offset, data)

from mercurial.i18n import _

import contextlib
import fcntl

class filetransaction(object):
    def __init__(self, report, opener):
        self.report = report
        self.opener = opener
        self.closed = False
        self.journalfile = 'filejournals'

    @contextlib.contextmanager
    def _lock(self, m='w'):
        f = self.opener.open(self.journalfile, m)
        try:
            fcntl.lockf(f, fcntl.LOCK_EX)
            yield f
        finally:
            f.close()

    def add(self, file, offset, data=None):
        with self._lock() as f:
            f.seek(0, 2)
            f.write('%d\0%s\n' % (offset, file))
            f.flush()

    def close(self):
        self.closed = True

    def _read(self):
        entries = []
        with self._lock(m='r+') as f:
            for line in f.readlines():
                e = line.split('\0', 1)
                entries.append((e[1].rstrip(), int(e[0])))
        return entries

    def abort(self):
        self.report(_('transaction abort!\n'))
        for filename, offset in self._read():
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


# changelog.py - changelog class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from node import bin, hex, nullid
from i18n import _
import util, error, revlog, encoding

def _string_escape(text):
    """
    >>> d = {'nl': chr(10), 'bs': chr(92), 'cr': chr(13), 'nul': chr(0)}
    >>> s = "ab%(nl)scd%(bs)s%(bs)sn%(nul)sab%(cr)scd%(bs)s%(nl)s" % d
    >>> s
    'ab\\ncd\\\\\\\\n\\x00ab\\rcd\\\\\\n'
    >>> res = _string_escape(s)
    >>> s == res.decode('string_escape')
    True
    """
    # subset of the string_escape codec
    text = text.replace('\\', '\\\\').replace('\n', '\\n').replace('\r', '\\r')
    return text.replace('\0', '\\0')

def decodeextra(text):
    extra = {}
    for l in text.split('\0'):
        if l:
            k, v = l.decode('string_escape').split(':', 1)
            extra[k] = v
    return extra

def encodeextra(d):
    # keys must be sorted to produce a deterministic changelog entry
    items = [_string_escape('%s:%s' % (k, d[k])) for k in sorted(d)]
    return "\0".join(items)

class appender(object):
    '''the changelog index must be updated last on disk, so we use this class
    to delay writes to it'''
    def __init__(self, fp, buf):
        self.data = buf
        self.fp = fp
        self.offset = fp.tell()
        self.size = util.fstat(fp).st_size

    def end(self):
        return self.size + len("".join(self.data))
    def tell(self):
        return self.offset
    def flush(self):
        pass
    def close(self):
        self.fp.close()

    def seek(self, offset, whence=0):
        '''virtual file offset spans real file and data'''
        if whence == 0:
            self.offset = offset
        elif whence == 1:
            self.offset += offset
        elif whence == 2:
            self.offset = self.end() + offset
        if self.offset < self.size:
            self.fp.seek(self.offset)

    def read(self, count=-1):
        '''only trick here is reads that span real file and data'''
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
            s = self.data[0][doff:doff+count]
            self.offset += len(s)
            ret += s
        return ret

    def write(self, s):
        self.data.append(str(s))
        self.offset += len(s)

class changelog(revlog.revlog):
    def __init__(self, opener):
        self._realopener = opener
        self._delayed = False
        revlog.revlog.__init__(self, self._delayopener, "00changelog.i")

    def delayupdate(self):
        "delay visibility of index updates to other readers"
        self._delayed = True
        self._delaycount = len(self)
        self._delaybuf = []
        self._delayname = None

    def finalize(self, tr):
        "finalize index updates"
        self._delayed = False
        # move redirected index data back into place
        if self._delayname:
            util.rename(self._delayname + ".a", self._delayname)
        elif self._delaybuf:
            fp = self.opener(self.indexfile, 'a')
            fp.write("".join(self._delaybuf))
            fp.close()
            self._delaybuf = []
        # split when we're done
        self.checkinlinesize(tr)

    def _delayopener(self, name, mode='r'):
        fp = self._realopener(name, mode)
        # only divert the index
        if not self._delayed or not name == self.indexfile:
            return fp
        # if we're doing an initial clone, divert to another file
        if self._delaycount == 0:
            self._delayname = fp.name
            if not len(self):
                # make sure to truncate the file
                mode = mode.replace('a', 'w')
            return self._realopener(name + ".a", mode)
        # otherwise, divert to memory
        return appender(fp, self._delaybuf)

    def readpending(self, file):
        r = revlog.revlog(self.opener, file)
        self.index = r.index
        self.nodemap = r.nodemap
        self._chunkcache = r._chunkcache

    def writepending(self):
        "create a file containing the unfinalized state for pretxnchangegroup"
        if self._delaybuf:
            # make a temporary copy of the index
            fp1 = self._realopener(self.indexfile)
            fp2 = self._realopener(self.indexfile + ".a", "w")
            fp2.write(fp1.read())
            # add pending data
            fp2.write("".join(self._delaybuf))
            fp2.close()
            # switch modes so finalize can simply rename
            self._delaybuf = []
            self._delayname = fp1.name

        if self._delayname:
            return True

        return False

    def checkinlinesize(self, tr, fp=None):
        if self.opener == self._delayopener:
            return
        return revlog.revlog.checkinlinesize(self, tr, fp)

    def read(self, node):
        """
        format used:
        nodeid\n        : manifest node in ascii
        user\n          : user, no \n or \r allowed
        time tz extra\n : date (time is int or float, timezone is int)
                        : extra is metadatas, encoded and separated by '\0'
                        : older versions ignore it
        files\n\n       : files modified by the cset, no \n or \r allowed
        (.*)            : comment (free text, ideally utf-8)

        changelog v0 doesn't use extra
        """
        text = self.revision(node)
        if not text:
            return (nullid, "", (0, 0), [], "", {'branch': 'default'})
        last = text.index("\n\n")
        desc = encoding.tolocal(text[last + 2:])
        l = text[:last].split('\n')
        manifest = bin(l[0])
        user = encoding.tolocal(l[1])

        extra_data = l[2].split(' ', 2)
        if len(extra_data) != 3:
            time = float(extra_data.pop(0))
            try:
                # various tools did silly things with the time zone field.
                timezone = int(extra_data[0])
            except:
                timezone = 0
            extra = {}
        else:
            time, timezone, extra = extra_data
            time, timezone = float(time), int(timezone)
            extra = decodeextra(extra)
        if not extra.get('branch'):
            extra['branch'] = 'default'
        files = l[3:]
        return (manifest, user, (time, timezone), files, desc, extra)

    def add(self, manifest, files, desc, transaction, p1, p2,
                  user, date=None, extra={}):
        user = user.strip()
        # An empty username or a username with a "\n" will make the
        # revision text contain two "\n\n" sequences -> corrupt
        # repository since read cannot unpack the revision.
        if not user:
            raise error.RevlogError(_("empty username"))
        if "\n" in user:
            raise error.RevlogError(_("username %s contains a newline")
                                    % repr(user))

        # strip trailing whitespace and leading and trailing empty lines
        desc = '\n'.join([l.rstrip() for l in desc.splitlines()]).strip('\n')

        user, desc = encoding.fromlocal(user), encoding.fromlocal(desc)

        if date:
            parseddate = "%d %d" % util.parsedate(date)
        else:
            parseddate = "%d %d" % util.makedate()
        if extra and extra.get("branch") in ("default", ""):
            del extra["branch"]
        if extra:
            extra = encodeextra(extra)
            parseddate = "%s %s" % (parseddate, extra)
        l = [hex(manifest), user, parseddate] + sorted(files) + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, len(self), p1, p2)

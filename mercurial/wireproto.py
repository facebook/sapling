# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import urllib, tempfile, os, sys
from i18n import _
from node import bin, hex
import changegroup as changegroupmod
import repo, error, encoding, util, store
import pushkey as pushkeymod

# list of nodes encoding / decoding

def decodelist(l, sep=' '):
    return map(bin, l.split(sep))

def encodelist(l, sep=' '):
    return sep.join(map(hex, l))

# client side

class wirerepository(repo.repository):
    def lookup(self, key):
        self.requirecap('lookup', _('look up remote revision'))
        d = self._call("lookup", key=encoding.fromlocal(key))
        success, data = d[:-1].split(" ", 1)
        if int(success):
            return bin(data)
        self._abort(error.RepoError(data))

    def heads(self):
        d = self._call("heads")
        try:
            return decodelist(d[:-1])
        except:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def branchmap(self):
        d = self._call("branchmap")
        try:
            branchmap = {}
            for branchpart in d.splitlines():
                branchname, branchheads = branchpart.split(' ', 1)
                branchname = encoding.tolocal(urllib.unquote(branchname))
                branchheads = decodelist(branchheads)
                branchmap[branchname] = branchheads
            return branchmap
        except TypeError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def branches(self, nodes):
        n = encodelist(nodes)
        d = self._call("branches", nodes=n)
        try:
            br = [tuple(decodelist(b)) for b in d.splitlines()]
            return br
        except:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def between(self, pairs):
        batch = 8 # avoid giant requests
        r = []
        for i in xrange(0, len(pairs), batch):
            n = " ".join([encodelist(p, '-') for p in pairs[i:i + batch]])
            d = self._call("between", pairs=n)
            try:
                r.extend(l and decodelist(l) or [] for l in d.splitlines())
            except:
                self._abort(error.ResponseError(_("unexpected response:"), d))
        return r

    def pushkey(self, namespace, key, old, new):
        if not self.capable('pushkey'):
            return False
        d = self._call("pushkey",
                       namespace=encoding.fromlocal(namespace),
                       key=encoding.fromlocal(key),
                       old=encoding.fromlocal(old),
                       new=encoding.fromlocal(new))
        try:
            d = bool(int(d))
        except ValueError:
            raise error.ResponseError(
                _('push failed (unexpected response):'), d)
        return d

    def listkeys(self, namespace):
        if not self.capable('pushkey'):
            return {}
        d = self._call("listkeys", namespace=encoding.fromlocal(namespace))
        r = {}
        for l in d.splitlines():
            k, v = l.split('\t')
            r[encoding.tolocal(k)] = encoding.tolocal(v)
        return r

    def stream_out(self):
        return self._callstream('stream_out')

    def changegroup(self, nodes, kind):
        n = encodelist(nodes)
        f = self._callstream("changegroup", roots=n)
        return changegroupmod.unbundle10(self._decompress(f), 'UN')

    def changegroupsubset(self, bases, heads, kind):
        self.requirecap('changegroupsubset', _('look up remote changes'))
        bases = encodelist(bases)
        heads = encodelist(heads)
        f = self._callstream("changegroupsubset",
                             bases=bases, heads=heads)
        return changegroupmod.unbundle10(self._decompress(f), 'UN')

    def unbundle(self, cg, heads, source):
        '''Send cg (a readable file-like object representing the
        changegroup to push, typically a chunkbuffer object) to the
        remote server as a bundle. Return an integer indicating the
        result of the push (see localrepository.addchangegroup()).'''

        ret, output = self._callpush("unbundle", cg, heads=encodelist(heads))
        if ret == "":
            raise error.ResponseError(
                _('push failed:'), output)
        try:
            ret = int(ret)
        except ValueError:
            raise error.ResponseError(
                _('push failed (unexpected response):'), ret)

        for l in output.splitlines(True):
            self.ui.status(_('remote: '), l)
        return ret

# server side

class streamres(object):
    def __init__(self, gen):
        self.gen = gen

class pushres(object):
    def __init__(self, res):
        self.res = res

class pusherr(object):
    def __init__(self, res):
        self.res = res

def dispatch(repo, proto, command):
    func, spec = commands[command]
    args = proto.getargs(spec)
    return func(repo, proto, *args)

def between(repo, proto, pairs):
    pairs = [decodelist(p, '-') for p in pairs.split(" ")]
    r = []
    for b in repo.between(pairs):
        r.append(encodelist(b) + "\n")
    return "".join(r)

def branchmap(repo, proto):
    branchmap = repo.branchmap()
    heads = []
    for branch, nodes in branchmap.iteritems():
        branchname = urllib.quote(encoding.fromlocal(branch))
        branchnodes = encodelist(nodes)
        heads.append('%s %s' % (branchname, branchnodes))
    return '\n'.join(heads)

def branches(repo, proto, nodes):
    nodes = decodelist(nodes)
    r = []
    for b in repo.branches(nodes):
        r.append(encodelist(b) + "\n")
    return "".join(r)

def capabilities(repo, proto):
    caps = 'lookup changegroupsubset branchmap pushkey'.split()
    if _allowstream(repo.ui):
        requiredformats = repo.requirements & repo.supportedformats
        # if our local revlogs are just revlogv1, add 'stream' cap
        if not requiredformats - set(('revlogv1',)):
            caps.append('stream')
        # otherwise, add 'streamreqs' detailing our local revlog format
        else:
            caps.append('streamreqs=%s' % ','.join(requiredformats))
    caps.append('unbundle=%s' % ','.join(changegroupmod.bundlepriority))
    return ' '.join(caps)

def changegroup(repo, proto, roots):
    nodes = decodelist(roots)
    cg = repo.changegroup(nodes, 'serve')
    return streamres(proto.groupchunks(cg))

def changegroupsubset(repo, proto, bases, heads):
    bases = decodelist(bases)
    heads = decodelist(heads)
    cg = repo.changegroupsubset(bases, heads, 'serve')
    return streamres(proto.groupchunks(cg))

def heads(repo, proto):
    h = repo.heads()
    return encodelist(h) + "\n"

def hello(repo, proto):
    '''the hello command returns a set of lines describing various
    interesting things about the server, in an RFC822-like format.
    Currently the only one defined is "capabilities", which
    consists of a line in the form:

    capabilities: space separated list of tokens
    '''
    return "capabilities: %s\n" % (capabilities(repo, proto))

def listkeys(repo, proto, namespace):
    d = pushkeymod.list(repo, encoding.tolocal(namespace)).items()
    t = '\n'.join(['%s\t%s' % (encoding.fromlocal(k), encoding.fromlocal(v))
                   for k, v in d])
    return t

def lookup(repo, proto, key):
    try:
        r = hex(repo.lookup(encoding.tolocal(key)))
        success = 1
    except Exception, inst:
        r = str(inst)
        success = 0
    return "%s %s\n" % (success, r)

def pushkey(repo, proto, namespace, key, old, new):
    # compatibility with pre-1.8 clients which were accidentally
    # sending raw binary nodes rather than utf-8-encoded hex
    if len(new) == 20 and new.encode('string-escape') != new:
        # looks like it could be a binary node
        try:
            u = new.decode('utf-8')
            new = encoding.tolocal(new) # but cleanly decodes as UTF-8
        except UnicodeDecodeError:
            pass # binary, leave unmodified
    else:
        new = encoding.tolocal(new) # normal path

    r = pushkeymod.push(repo,
                        encoding.tolocal(namespace), encoding.tolocal(key),
                        encoding.tolocal(old), new)
    return '%s\n' % int(r)

def _allowstream(ui):
    return ui.configbool('server', 'uncompressed', True, untrusted=True)

def stream(repo, proto):
    '''If the server supports streaming clone, it advertises the "stream"
    capability with a value representing the version and flags of the repo
    it is serving. Client checks to see if it understands the format.

    The format is simple: the server writes out a line with the amount
    of files, then the total amount of bytes to be transfered (separated
    by a space). Then, for each file, the server first writes the filename
    and filesize (separated by the null character), then the file contents.
    '''

    if not _allowstream(repo.ui):
        return '1\n'

    entries = []
    total_bytes = 0
    try:
        # get consistent snapshot of repo, lock during scan
        lock = repo.lock()
        try:
            repo.ui.debug('scanning\n')
            for name, ename, size in repo.store.walk():
                entries.append((name, size))
                total_bytes += size
        finally:
            lock.release()
    except error.LockError:
        return '2\n' # error: 2

    def streamer(repo, entries, total):
        '''stream out all metadata files in repository.'''
        yield '0\n' # success
        repo.ui.debug('%d files, %d bytes to transfer\n' %
                      (len(entries), total_bytes))
        yield '%d %d\n' % (len(entries), total_bytes)
        for name, size in entries:
            repo.ui.debug('sending %s (%d bytes)\n' % (name, size))
            # partially encode name over the wire for backwards compat
            yield '%s\0%d\n' % (store.encodedir(name), size)
            for chunk in util.filechunkiter(repo.sopener(name), limit=size):
                yield chunk

    return streamres(streamer(repo, entries, total_bytes))

def unbundle(repo, proto, heads):
    their_heads = decodelist(heads)

    def check_heads():
        heads = repo.heads()
        return their_heads == ['force'] or their_heads == heads

    proto.redirect()

    # fail early if possible
    if not check_heads():
        return pusherr('unsynced changes')

    # write bundle data to temporary file because it can be big
    fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
    fp = os.fdopen(fd, 'wb+')
    r = 0
    try:
        proto.getfile(fp)
        lock = repo.lock()
        try:
            if not check_heads():
                # someone else committed/pushed/unbundled while we
                # were transferring data
                return pusherr('unsynced changes')

            # push can proceed
            fp.seek(0)
            gen = changegroupmod.readbundle(fp, None)

            try:
                r = repo.addchangegroup(gen, 'serve', proto._client(),
                                        lock=lock)
            except util.Abort, inst:
                sys.stderr.write("abort: %s\n" % inst)
        finally:
            lock.release()
        return pushres(r)

    finally:
        fp.close()
        os.unlink(tempname)

commands = {
    'between': (between, 'pairs'),
    'branchmap': (branchmap, ''),
    'branches': (branches, 'nodes'),
    'capabilities': (capabilities, ''),
    'changegroup': (changegroup, 'roots'),
    'changegroupsubset': (changegroupsubset, 'bases heads'),
    'heads': (heads, ''),
    'hello': (hello, ''),
    'listkeys': (listkeys, 'namespace'),
    'lookup': (lookup, 'key'),
    'pushkey': (pushkey, 'namespace key old new'),
    'stream_out': (stream, ''),
    'unbundle': (unbundle, 'heads'),
}

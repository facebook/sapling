# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import urllib, tempfile, os
from i18n import _
from node import bin, hex
import changegroup as changegroupmod
import streamclone, repo, error, encoding, util
import pushkey as pushkey_

# list of nodes encoding / decoding

def decodelist(l, sep=' '):
    return map(bin, l.split(sep))

def encodelist(l, sep=' '):
    return sep.join(map(hex, l))

# client side

class wirerepository(repo.repository):
    def lookup(self, key):
        self.requirecap('lookup', _('look up remote revision'))
        d = self._call("lookup", key=key)
        success, data = d[:-1].split(" ", 1)
        if int(success):
            return bin(data)
        self._abort(error.RepoError(data))

    def heads(self):
        d = self._call("heads")
        try:
            return decodelist(d[:-1])
        except:
            self.abort(error.ResponseError(_("unexpected response:"), d))

    def branchmap(self):
        d = self._call("branchmap")
        try:
            branchmap = {}
            for branchpart in d.splitlines():
                branchname, branchheads = branchpart.split(' ', 1)
                branchname = urllib.unquote(branchname)
                # Earlier servers (1.3.x) send branch names in (their) local
                # charset. The best we can do is assume it's identical to our
                # own local charset, in case it's not utf-8.
                try:
                    branchname.decode('utf-8')
                except UnicodeDecodeError:
                    branchname = encoding.fromlocal(branchname)
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
                      namespace=namespace, key=key, old=old, new=new)
        return bool(int(d))

    def listkeys(self, namespace):
        if not self.capable('pushkey'):
            return {}
        d = self._call("listkeys", namespace=namespace)
        r = {}
        for l in d.splitlines():
            k, v = l.split('\t')
            r[k.decode('string-escape')] = v.decode('string-escape')
        return r

    def stream_out(self):
        return self._callstream('stream_out')

    def changegroup(self, nodes, kind):
        n = encodelist(nodes)
        f = self._callstream("changegroup", roots=n)
        return self._decompress(f)

    def changegroupsubset(self, bases, heads, kind):
        self.requirecap('changegroupsubset', _('look up remote changes'))
        bases = encodelist(bases)
        heads = encodelist(heads)
        return self._decompress(self._callstream("changegroupsubset",
                                                 bases=bases, heads=heads))

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
        except ValueError, err:
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
        branchname = urllib.quote(branch)
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
    if streamclone.allowed(repo.ui):
        caps.append('stream=%d' % repo.changelog.version)
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
    d = pushkey_.list(repo, namespace).items()
    t = '\n'.join(['%s\t%s' % (k.encode('string-escape'),
                               v.encode('string-escape')) for k, v in d])
    return t

def lookup(repo, proto, key):
    try:
        r = hex(repo.lookup(key))
        success = 1
    except Exception, inst:
        r = str(inst)
        success = 0
    return "%s %s\n" % (success, r)

def pushkey(repo, proto, namespace, key, old, new):
    r = pushkey_.push(repo, namespace, key, old, new)
    return '%s\n' % int(r)

def stream(repo, proto):
    return streamres(streamclone.stream_out(repo))

def unbundle(repo, proto, heads):
    their_heads = decodelist(heads)

    def check_heads():
        heads = repo.heads()
        return their_heads == ['force'] or their_heads == heads

    # fail early if possible
    if not check_heads():
        return 'unsynced changes'

    # write bundle data to temporary file because it can be big
    fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
    fp = os.fdopen(fd, 'wb+')
    r = 0
    proto.redirect()
    try:
        proto.getfile(fp)
        lock = repo.lock()
        try:
            if not check_heads():
                # someone else committed/pushed/unbundled while we
                # were transferring data
                return 'unsynced changes'

            # push can proceed
            fp.seek(0)
            header = fp.read(6)
            if header.startswith('HG'):
                if not header.startswith('HG10'):
                    raise ValueError('unknown bundle version')
                elif header not in changegroupmod.bundletypes:
                    raise ValueError('unknown bundle compression type')
            gen = changegroupmod.unbundle(header, fp)

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

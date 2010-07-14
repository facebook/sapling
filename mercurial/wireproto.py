# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import bin, hex
import urllib
import streamclone, repo, error, encoding
import pushkey as pushkey_

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
            return map(bin, d[:-1].split(" "))
        except:
            self.abort(error.ResponseError(_("unexpected response:"), d))

    def branchmap(self):
        d = self._call("branchmap")
        try:
            branchmap = {}
            for branchpart in d.splitlines():
                branchheads = branchpart.split(' ')
                branchname = urllib.unquote(branchheads[0])
                # Earlier servers (1.3.x) send branch names in (their) local
                # charset. The best we can do is assume it's identical to our
                # own local charset, in case it's not utf-8.
                try:
                    branchname.decode('utf-8')
                except UnicodeDecodeError:
                    branchname = encoding.fromlocal(branchname)
                branchheads = [bin(x) for x in branchheads[1:]]
                branchmap[branchname] = branchheads
            return branchmap
        except TypeError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self._call("branches", nodes=n)
        try:
            br = [tuple(map(bin, b.split(" "))) for b in d.splitlines()]
            return br
        except:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def between(self, pairs):
        batch = 8 # avoid giant requests
        r = []
        for i in xrange(0, len(pairs), batch):
            n = " ".join(["-".join(map(hex, p)) for p in pairs[i:i + batch]])
            d = self._call("between", pairs=n)
            try:
                r += [l and map(bin, l.split(" ")) or []
                      for l in d.splitlines()]
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

# server side

def dispatch(repo, proto, command):
    if command not in commands:
        return False
    func, spec = commands[command]
    args = proto.getargs(spec)
    r = func(repo, proto, *args)
    if r != None:
        proto.respond(r)
    return True

def between(repo, proto, pairs):
    pairs = [map(bin, p.split("-")) for p in pairs.split(" ")]
    r = []
    for b in repo.between(pairs):
        r.append(" ".join(map(hex, b)) + "\n")
    return "".join(r)

def branchmap(repo, proto):
    branchmap = repo.branchmap()
    heads = []
    for branch, nodes in branchmap.iteritems():
        branchname = urllib.quote(branch)
        branchnodes = [hex(node) for node in nodes]
        heads.append('%s %s' % (branchname, ' '.join(branchnodes)))
    return '\n'.join(heads)

def branches(repo, proto, nodes):
    nodes = map(bin, nodes.split(" "))
    r = []
    for b in repo.branches(nodes):
        r.append(" ".join(map(hex, b)) + "\n")
    return "".join(r)

def changegroup(repo, proto, roots):
    nodes = map(bin, roots.split(" "))
    cg = repo.changegroup(nodes, 'serve')
    proto.sendchangegroup(cg)

def changegroupsubset(repo, proto, bases, heads):
    bases = [bin(n) for n in bases.split(' ')]
    heads = [bin(n) for n in heads.split(' ')]
    cg = repo.changegroupsubset(bases, heads, 'serve')
    proto.sendchangegroup(cg)

def heads(repo, proto):
    h = repo.heads()
    return " ".join(map(hex, h)) + "\n"

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
    try:
        proto.sendstream(streamclone.stream_out(repo))
    except streamclone.StreamException, inst:
        return str(inst)

commands = {
    'between': (between, 'pairs'),
    'branchmap': (branchmap, ''),
    'branches': (branches, 'nodes'),
    'changegroup': (changegroup, 'roots'),
    'changegroupsubset': (changegroupsubset, 'bases heads'),
    'heads': (heads, ''),
    'listkeys': (listkeys, 'namespace'),
    'lookup': (lookup, 'key'),
    'pushkey': (pushkey, 'namespace key old new'),
    'stream_out': (stream, ''),
}

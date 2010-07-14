# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import bin, hex
import urllib
import pushkey as pushkey_

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
}

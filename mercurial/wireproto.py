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
    proto.respond(func(repo, *args))
    return True

def between(repo, pairs):
    pairs = [map(bin, p.split("-")) for p in pairs.split(" ")]
    r = []
    for b in repo.between(pairs):
        r.append(" ".join(map(hex, b)) + "\n")
    return "".join(r)

def branchmap(repo):
    branchmap = repo.branchmap()
    heads = []
    for branch, nodes in branchmap.iteritems():
        branchname = urllib.quote(branch)
        branchnodes = [hex(node) for node in nodes]
        heads.append('%s %s' % (branchname, ' '.join(branchnodes)))
    return '\n'.join(heads)

def branches(repo, nodes):
    nodes = map(bin, nodes.split(" "))
    r = []
    for b in repo.branches(nodes):
        r.append(" ".join(map(hex, b)) + "\n")
    return "".join(r)

def heads(repo):
    h = repo.heads()
    return " ".join(map(hex, h)) + "\n"

def listkeys(repo, namespace):
    d = pushkey_.list(repo, namespace).items()
    t = '\n'.join(['%s\t%s' % (k.encode('string-escape'),
                               v.encode('string-escape')) for k, v in d])
    return t

def lookup(repo, key):
    try:
        r = hex(repo.lookup(key))
        success = 1
    except Exception, inst:
        r = str(inst)
        success = 0
    return "%s %s\n" % (success, r)

def pushkey(repo, namespace, key, old, new):
    r = pushkey_.push(repo, namespace, key, old, new)
    return '%s\n' % int(r)

commands = {
    'between': (between, 'pairs'),
    'branchmap': (branchmap, ''),
    'branches': (branches, 'nodes'),
    'heads': (heads, ''),
    'listkeys': (listkeys, 'namespace'),
    'lookup': (lookup, 'key'),
    'pushkey': (pushkey, 'namespace key old new'),
}

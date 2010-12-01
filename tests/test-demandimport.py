from mercurial import demandimport
demandimport.enable()

import re

rsub = re.sub
def f(obj):
    l = repr(obj)
    l = rsub("0x[0-9a-fA-F]+", "0x?", l)
    l = rsub("from '.*'", "from '?'", l)
    l = rsub("'<[a-z]*>'", "'<whatever>'", l)
    return l

import os

print "os =", f(os)
print "os.system =", f(os.system)
print "os =", f(os)

from mercurial import util

print "util =", f(util)
print "util.system =", f(util.system)
print "util =", f(util)
print "util.system =", f(util.system)

import re as fred
print "fred =", f(fred)

import sys as re
print "re =", f(re)

print "fred =", f(fred)
print "fred.sub =", f(fred.sub)
print "fred =", f(fred)

print "re =", f(re)
print "re.stderr =", f(re.stderr)
print "re =", f(re)

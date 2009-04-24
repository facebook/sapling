#!/usr/bin/env python

from mercurial.hg import parseurl

def testparse(url, rev=[]):
    print '%s, revs: %r, checkout: %r' % parseurl(url, rev)

testparse('http://example.com/no/anchor')
testparse('http://example.com/an/anchor#foo')
testparse('http://example.com/no/anchor/revs', rev=['foo'])
testparse('http://example.com/an/anchor/revs#bar', rev=['foo'])
testparse('http://example.com/an/anchor/rev-None#foo', rev=None)

from __future__ import absolute_import, print_function

from mercurial import (
    hg,
)

def testparse(url, branch=[]):
    print('%s, branches: %r' % hg.parseurl(url, branch))

testparse('http://example.com/no/anchor')
testparse('http://example.com/an/anchor#foo')
testparse('http://example.com/no/anchor/branches', branch=['foo'])
testparse('http://example.com/an/anchor/branches#bar', branch=['foo'])
testparse('http://example.com/an/anchor/branches-None#foo', branch=None)
testparse('http://example.com/')
testparse('http://example.com')
testparse('http://example.com#foo')

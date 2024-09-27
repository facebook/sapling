from __future__ import absolute_import, print_function

from sapling import hg


def testparse(url):
    print("%s" % hg.parseurl(url))


testparse("http://example.com/no/anchor")
testparse("http://example.com/an/anchor#foo")
testparse("http://example.com/no/anchor/branches")
testparse("http://example.com/an/anchor/branches#bar")
testparse("http://example.com/an/anchor/branches-None#foo")
testparse("http://example.com/")
testparse("http://example.com")
testparse("http://example.com#foo")

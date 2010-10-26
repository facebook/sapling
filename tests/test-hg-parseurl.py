from mercurial.hg import parseurl

def testparse(url, branch=[]):
    print '%s, branches: %r' % parseurl(url, branch)

testparse('http://example.com/no/anchor')
testparse('http://example.com/an/anchor#foo')
testparse('http://example.com/no/anchor/branches', branch=['foo'])
testparse('http://example.com/an/anchor/branches#bar', branch=['foo'])
testparse('http://example.com/an/anchor/branches-None#foo', branch=None)

from mercurial import util

def printifpresent(d, xs):
    for x in xs:
        present = x in d
        print "'%s' in d: %s" % (x, present)
        if present:
            print "d['%s']: %s" % (x, d[x])

def test_lrucachedict():
    d = util.lrucachedict(4)
    d['a'] = 'va'
    d['b'] = 'vb'
    d['c'] = 'vc'
    d['d'] = 'vd'

    # all of these should be present
    printifpresent(d, ['a', 'b', 'c', 'd'])

    # 'a' should be dropped because it was least recently used
    d['e'] = 've'
    printifpresent(d, ['a', 'b', 'c', 'd', 'e'])

    # touch entries in some order (get or set).
    d['e']
    d['c'] = 'vc2'
    d['d']
    d['b'] = 'vb2'

    # 'e' should be dropped now
    d['f'] = 'vf'
    printifpresent(d, ['b', 'c', 'd', 'e', 'f'])

    d.clear()
    printifpresent(d, ['b', 'c', 'd', 'e', 'f'])

if __name__ == '__main__':
    test_lrucachedict()

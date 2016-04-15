from __future__ import absolute_import, print_function

from mercurial import (
    util,
)

def printifpresent(d, xs, name='d'):
    for x in xs:
        present = x in d
        print("'%s' in %s: %s" % (x, name, present))
        if present:
            print("%s['%s']: %s" % (name, x, d[x]))

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

    # Now test dicts that aren't full.
    d = util.lrucachedict(4)
    d['a'] = 1
    d['b'] = 2
    d['a']
    d['b']
    printifpresent(d, ['a', 'b'])

    # test copy method
    d = util.lrucachedict(4)
    d['a'] = 'va3'
    d['b'] = 'vb3'
    d['c'] = 'vc3'
    d['d'] = 'vd3'

    dc = d.copy()

    # all of these should be present
    print("\nAll of these should be present:")
    printifpresent(dc, ['a', 'b', 'c', 'd'], 'dc')

    # 'a' should be dropped because it was least recently used
    print("\nAll of these except 'a' should be present:")
    dc['e'] = 've3'
    printifpresent(dc, ['a', 'b', 'c', 'd', 'e'], 'dc')

    # contents and order of original dict should remain unchanged
    print("\nThese should be in reverse alphabetical order and read 'v?3':")
    dc['b'] = 'vb3_new'
    for k in list(iter(d)):
        print("d['%s']: %s" % (k, d[k]))

if __name__ == '__main__':
    test_lrucachedict()

#!/usr/bin/python
import sys, os, re

IGNORE = ['.css', '.py']
oldre = re.compile('#([\w\|%]+)#')

def rewrite(fn):
    f = open(fn)
    new = open(fn + '.new', 'wb')
    for ln in f:
        new.write(oldre.sub('{\\1}', ln))
    new.close()
    f.close()
    os.rename(new.name, f.name)

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print 'usage: python tmplrewrite.py [file [file [file]]]'
    for fn in sys.argv[1:]:
        if os.path.splitext(fn) in IGNORE:
            continue
        print 'rewriting %s...' % fn
        rewrite(fn)

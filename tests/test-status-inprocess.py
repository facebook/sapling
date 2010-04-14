#!/usr/bin/python
from mercurial.ui import ui
from mercurial.localrepo import localrepository
from mercurial.commands import add, commit, status

u = ui()

print '% creating repo'
repo = localrepository(u, '.', create=True)

f = open('test.py', 'w')
try:
    f.write('foo\n')
finally:
    f.close

print '% add and commit'
add(u, repo, 'test.py')
commit(u, repo, message='*')
status(u, repo, clean=True)


print '% change'
f = open('test.py', 'w')
try:
    f.write('bar\n')
finally:
    f.close()

# this would return clean instead of changed before the fix
status(u, repo, clean=True, modified=True)

import os
from mercurial import hg, ui, merge

u = ui.ui()

repo = hg.repository(u, 'test1', create=1)
os.chdir('test1')

def commit(text, time):
    repo.commit(text=text, date="%d 0" % time)

def addcommit(name, time):
    f = open(name, 'w')
    f.write('%s\n' % name)
    f.close()
    repo[None].add([name])
    commit(name, time)

def update(rev):
    merge.update(repo, rev, False, True, False)

def merge_(rev):
    merge.update(repo, rev, True, False, False)

if __name__ == '__main__':
    addcommit("A", 0)
    addcommit("B", 1)

    update(0)
    addcommit("C", 2)

    merge_(1)
    commit("D", 3)

    update(2)
    addcommit("E", 4)
    addcommit("F", 5)

    update(3)
    addcommit("G", 6)

    merge_(5)
    commit("H", 7)

    update(5)
    addcommit("I", 8)

    # Ancestors
    print 'Ancestors of 5'
    for r in repo.changelog.ancestors([5]):
        print r,

    print '\nAncestors of 6 and 5'
    for r in repo.changelog.ancestors([6, 5]):
        print r,

    print '\nAncestors of 5 and 4'
    for r in repo.changelog.ancestors([5, 4]):
        print r,

    print '\nAncestors of 7, stop at 6'
    for r in repo.changelog.ancestors([7], 6):
        print r,

    print '\nAncestors of 7, including revs'
    for r in repo.changelog.ancestors([7], inclusive=True):
        print r,

    print '\nAncestors of 7, 5 and 3, including revs'
    for r in repo.changelog.ancestors([7, 5, 3], inclusive=True):
        print r,

    # Descendants
    print '\n\nDescendants of 5'
    for r in repo.changelog.descendants([5]):
        print r,

    print '\nDescendants of 5 and 3'
    for r in repo.changelog.descendants([5, 3]):
        print r,

    print '\nDescendants of 5 and 4'
    for r in repo.changelog.descendants([5, 4]):
        print r,


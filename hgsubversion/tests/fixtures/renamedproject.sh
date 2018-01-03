#!/bin/sh
#
# Convert a project moving from a non-canonical to canonical
# layout, exercizing the missing plaintext code paths. It also tests
# branch creations where the branch source is not a canonical branch.
#

mkdir temp
cd temp

svnadmin create testrepo
svnurl=file://`pwd`/testrepo

mkdir project-orig
cd project-orig
echo a > a
echo b > b
echo c > c
mkdir d
echo a > d/a
cd ..

# Let's suppose it was actually branched in a previous life
mkdir project-branch
cd project-branch
echo a > a
echo b > b
cd ..

svn import project-orig $svnurl/project-orig -m "init project"
svn import project-branch $svnurl/project-branch -m "init branch"

svn mkdir $svnurl/project -m "create new project hierarchy"
svn mv $svnurl/project-orig $svnurl/project/project -m "rename as project"
svn mv $svnurl/project/project $svnurl/project/trunk -m "rename as project"

svn mkdir $svnurl/project/branches -m "add branches root"
svn mv $svnurl/project-branch $svnurl/project/misplaced -m "incorrect move of the branch"
svn mv $svnurl/project/misplaced $svnurl/project/branches/branch -m "move of the branch"

svn co $svnurl/project
cd project
echo a >> trunk/a
svn ci -m "change a"
echo a >> trunk/a
echo b >> trunk/b
svn rm trunk/c
echo a >> trunk/d/a
svn ci -m "change files in trunk"
# Try the same thing with the branch
echo a >> branches/branch/a
svn rm branches/branch/b
svn ci -m "change a in branch"
cd ..

# Add this to make test_rebuildmeta happy, needs something to convert
svn import project-orig $svnurl/trunk -m "init fake trunk for rebuild_meta"

svnadmin dump testrepo > ../renamedproject.svndump

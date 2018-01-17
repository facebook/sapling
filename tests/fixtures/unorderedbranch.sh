#!/bin/sh
#
# Generate unorderedbranch.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/trunk
echo a > a
svn add a
svn ci -m "add a in trunk"
echo b > b
echo z > z
svn add b z
svn ci -m "add b and z in trunk"
svn up
cd ../branches
# Copy from trunk past revision. The converted used to take the last
# trunk revision as branch parent instead of the specified one.
svn cp -r 2 ../trunk branch
svn cp ../trunk/z branch
echo c > branch/c
svn add branch/c
svn ci -m 'branch and add c'
cd ../..

svnadmin dump testrepo > ../unorderedbranch.svndump

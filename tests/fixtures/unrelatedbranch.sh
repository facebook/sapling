#!/bin/sh
#
# Generate unrelatedbranch.svndump
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
cd ../branches
# Create an unrelated branch with another file. It used to lead the converter
# to think branch1 was a copy of trunk, even without copy information.
mkdir branch1
echo b > branch1/b
svn add branch1
svn ci -m "add b in branch1"
# Make a real branch too for comparison
svn cp ../trunk branch2
echo b > branch2/b
svn add branch2/b
svn ci -m "add b to branch2"
# Add a file in the branch root for fun
echo c > c
svn add c
svn ci -m "add c in branches/"
cd ../..

svnadmin dump testrepo > ../unrelatedbranch.svndump

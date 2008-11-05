#!/bin/sh
#
# Generate renames.svndump
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
echo b > b
mkdir -p da/db
echo c > da/daf
echo d > da/db/dbf
svn add a b da
svn ci -m "add a and b"
cd ../branches
svn cp ../trunk branch1
svn ci -m "create branch1"
cd branch1
echo c > c
svn add c
svn ci -m "add c"
cd ../../trunk
# Regular copy and rename
svn cp a a1
svn mv a a2
# Copy and update of source and dest
svn cp b b1
echo b >> b
echo c >> b1
# Directory copy and renaming
svn cp da da1
svn mv da da2
# Test one copy operation in branch
cd ../branches/branch1
svn cp c c1
echo c >> c1
cd ../..
svn ci -m "rename and copy a, b and da"
cd trunk
# Copy across branch
svn cp ../branches/branch1/c c
svn ci -m "copy b from branch1"
cd ../..

svnadmin dump testrepo > ../renames.svndump

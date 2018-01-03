#!/bin/sh
#
# Generate pushrenames.svndump
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
echo c > c
echo d > d
echo e > e
svn add a b c d e
svn ci -m "add files"
cd ../..

svnadmin dump testrepo > ../pushrenames.svndump

#!/bin/sh
#
# Generate truncatedhistory.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir project1
mkdir project2
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
# Make a single revision in trunk
cd project/project1
echo a > a
svn add a
svn ci -m "add a"
cd ..
svn up
# Rename the project
svn mv project1 project2/trunk
svn ci -m "rename project1"
cd project2/trunk
echo b > b
svn add b
svn ci -m "add b"
cd ../../..

svnadmin dump testrepo > ../truncatedhistory.svndump

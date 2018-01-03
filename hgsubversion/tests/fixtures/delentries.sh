#!/bin/sh
#
# Generate delentries.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/trunk
# Regular file deletion
echo a > a
# Another file starting like the deleted file
echo aa > aa
mkdir d1
mkdir d1/d2
mkdir d1/d2/d3
echo c > d1/c
# Test directory deletion
echo d > d1/d2/c
# Test subdirectory deletion
echo e > d1/d2/d3/e
echo f > d1/d2/d3/f
# This file starts as the deleted directory, can be confusing
echo d2prefix > d1/d2prefix
svn add a aa d1
svn ci -m "add entries"
svn rm a d1/d2
svn ci -m "remove entries"
cd ../..

svnadmin dump testrepo > ../delentries.svndump

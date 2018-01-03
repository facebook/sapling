#!/bin/sh
#
# Generate revert.svndump
#

rm -rf temp
mkdir temp
cd temp
mkdir -p import/trunk/dir
cd import/trunk
echo a > a
echo b > dir/b
cd ../..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import import $svnurl -m init

svn co $svnurl project
cd project
echo a >> trunk/a
echo b >> trunk/dir/b
svn ci -m changefiles
svn up
# Test directory revert
svn rm trunk
svn cp $svnurl/trunk@1 trunk
svn st
svn ci -m revert
svn up
# Test file revert
svn rm trunk/a
svn rm trunk/dir/b
svn cp $svnurl/trunk/a@2 trunk/a
svn cp $svnurl/trunk/dir/b@2 trunk/dir/b
svn ci -m revert2
cd ..

svnadmin dump testrepo > ../revert.svndump

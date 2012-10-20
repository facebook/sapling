#!/bin/sh
#
# Generate copies.svndump
#

rm -rf temp
mkdir temp
cd temp
mkdir -p import/trunk/dir
echo a > import/trunk/dir/a

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import import $svnurl -m init

svn co $svnurl project
cd project
svn cp trunk/dir trunk/dir2
echo b >> trunk/dir2/a
svn ci -m 'copy/edit trunk/dir/a'
svn up
svn cp trunk/dir2 trunk/dir3
svn ci -m 'copy dir2 to dir3'
cd ..

svnadmin dump testrepo > ../copies.svndump

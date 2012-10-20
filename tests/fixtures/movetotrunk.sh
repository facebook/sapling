#!/bin/sh
#
# Generate movetotrunk.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn mkdir --parents $svnurl/sub1/sub2 -m subpaths
svn import project-orig $svnurl/sub1/sub2 -m "init project"

svn co $svnurl/sub1/sub2 project
cd project
echo a > a
svn add a
mkdir dir
echo b > dir/b
svn add dir
svn ci -m adda
svn up
mkdir trunk
svn add trunk
svn mv a trunk/a
svn mv dir trunk/dir
svn ci -m 'move to trunk'
cd ..

svnadmin dump testrepo > ../movetotrunk.svndump

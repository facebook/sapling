#!/bin/sh
#
# Generate mergeexternals.svndump
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
mkdir d1
echo a > d1/a
mkdir d2
echo b > d2/b
mkdir -p common/ext
echo c > common/ext/c
svn add d1 d2 common
svn ci -m addfiles
svn up
svn propset svn:externals '^/trunk/common/ext ext' d1
svn propset svn:externals '^/trunk/common/ext ext' d2
svn ci -m addexternals
cd ..
svn up
svn cp trunk branches/branch
cd branches
svn ci -m addbranch
cd branch
mkdir d3
echo d > d3/d
svn add d3
svn propset svn:externals '^/trunk/common/ext ext3' d3
svn ci -m touchbranch
cd ../../trunk
svn merge '^/branches/branch'
svn up
svn ci -m 'merge'
cd ../..

svnadmin dump testrepo > ../mergeexternals.svndump

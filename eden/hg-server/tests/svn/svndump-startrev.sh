#!/bin/sh
#
# Use this script to generate startrev.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
mkdir tags
cd ..

svnadmin create svn-repo
svnurl=file://`pwd`/svn-repo
svn import project-orig $svnurl -m "init projA"

svn co $svnurl project
cd project
echo a > trunk/a
echo b > trunk/b
svn add trunk/a trunk/b
svn ci -m createab
svn rm trunk/b
svn ci -m removeb
svn up
echo a >> trunk/a
svn ci -m changeaa

# Branch
svn up
svn copy trunk branches/branch1
echo a >> branches/branch1/a
svn ci -m "branch, changeaaa"

echo a >> branches/branch1/a
echo c > branches/branch1/c
svn add branches/branch1/c
svn ci -m "addc,changeaaaa"
svn up
cd ..

svnadmin dump svn-repo > ../startrev.svndump
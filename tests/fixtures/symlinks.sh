#!/bin/sh
#
# Generate symlinks.svndump
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
ln -s a linka
mkdir d
ln -s a d/linka
svn add a linka d
svn ci -m "add symlinks"
# Move symlinks
svn mv linka linkaa
svn mv d d2
svn commit -m "moving symlinks"
# Update symlinks (test "link " prefix vs applydelta)
echo b > b
rm linkaa
ln -s b linkaa
rm d2/linka
ln -s b d2/linka
svn ci -m "update symlinks"
cd ../..

svnadmin dump testrepo > ../symlinks.svndump

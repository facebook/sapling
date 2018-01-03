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
# Create and remove a file, hgsubversion does not like
# empty repositories
echo a > a
svn add a
svn ci -m "add a"
svn rm a
svn ci -m "remove a"
cd ../..

svnadmin dump testrepo > ../emptyrepo.svndump

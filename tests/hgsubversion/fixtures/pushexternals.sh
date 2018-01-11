#!/bin/sh
#
# Generate pushexternals.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir externals
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/externals
mkdir project1
echo a > project1/a
svn add project1
mkdir project2
echo a > project2/b
svn add project2
svn ci -m "configure externals projects"
cd ../trunk
echo a > a
# dir is used to set svn:externals on an already existing directory
mkdir dir
svn add a dir
svn ci -m "add a and dir"
svn rm a
svn ci -m "remove a"
cd ../..

svnadmin dump testrepo > ../pushexternals.svndump

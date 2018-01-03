#!/bin/sh
#
# Generate filecase.svndump
# WARNING: this script must be run on a case-sensitive file system
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
# Test files and directories differing in case only
echo a > a
echo A > A
echo b > b
mkdir d
echo a > d/a
mkdir D
echo a > D/a
mkdir e
echo a > e/a
mkdir f
echo a > f/a
echo F > F
svn add a A b d D e f F
svn ci -m 'add files'
# Rename files and directories, changing only their case
svn mv b B
svn mv d/a d/A
svn mv e E
svn ci -m 'change case'
cd ../..

svnadmin dump testrepo > ../filecase.svndump

#!/bin/sh
#
# Generate simple_branch.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk branches tags
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import --username durin project-orig $svnurl -m "Empty dirs."

svn co $svnurl project
cd project
echo 'file: alpha' > trunk/alpha
svn add trunk/alpha
svn ci --username durin -m 'Add alpha'
echo 'Data of beta' > trunk/beta
svn add trunk/beta
svn ci --username durin -m 'Add beta'
svn up
svn cp trunk branches/the_branch
svn ci --username durin -m 'Make a branch'
cd ..

svnadmin dump testrepo > ../simple_branch.svndump

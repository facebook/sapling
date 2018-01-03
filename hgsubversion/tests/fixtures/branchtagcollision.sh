#!/bin/bash
#
# Generate branchtagcollision.svndump
#
# Generates an svn repository with a branch and a tag that have the same name.


mkdir temp
cd temp

svnadmin create testrepo
svn checkout file://`pwd`/testrepo client

cd client
mkdir trunk
mkdir branches
mkdir tags

svn add trunk branches tags
svn commit -m "Initial commit"

echo "fileA" >> trunk/fileA
svn add trunk/fileA
svn commit -m "Added fileA"

svn cp trunk branches/A
svn commit -m "added branch"

echo "fileB" >> trunk/fileB
svn add trunk/fileB
svn commit -m "Added fileB"

svn cp trunk tags/A
svn commit -m "added bad tag"

cd ..
svnadmin dump testrepo > ../branchtagcollision.svndump

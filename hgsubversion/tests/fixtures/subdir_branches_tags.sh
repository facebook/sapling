#!/usr/bin/env bash

set -e

mkdir temp
cd temp

svnadmin create testrepo
svn checkout file://`pwd`/testrepo client

cd client
mkdir trunk
mkdir -p bran/ches
mkdir -p ta/gs

svn add trunk bran ta
svn commit -m "Initial commit"

echo "trunk" >> trunk/file
svn add trunk/file
svn commit -m "Added file in trunk"

svn cp trunk ta/gs/tag_from_trunk
svn ci -m 'created tag from trunk'

svn cp trunk bran/ches/branch
svn ci -m 'created branch from trunk'

echo "branch" > bran/ches/branch/file
svn ci -m "committed to the branch"

svn cp bran/ches/branch ta/gs/tag_from_branch
svn ci -m "create tag from branch"

cd ..
svnadmin dump testrepo > ../subdir_branches_tags.svndump

echo "Created subdir_branches_tags.svndump"
echo "You might want to clean up ${PWD} now"

#!/usr/bin/env bash

set -e

mkdir temp
cd temp

svnadmin create testrepo
svn checkout file://`pwd`/testrepo client

cd client
mkdir trunk
mkdir branchez
mkdir tagz

svn add trunk branchez tagz
svn commit -m "Initial commit"

echo "trunk" >> trunk/file
svn add trunk/file
svn commit -m "Added file in trunk"

svn cp trunk tagz/tag_from_trunk
svn ci -m 'created tag from trunk'

svn cp trunk branchez/branch
svn ci -m 'created branch from trunk'

echo "branch" > branchez/branch/file
svn ci -m "committed to the branch"

svn cp branchez/branch tagz/tag_from_branch
svn ci -m "create tag from branch"

cd ..
svnadmin dump testrepo > ../misspelled_branches_tags.svndump

echo "Created misspelled_branches_tags.svndump"
echo "You might want to clean up ${PWD} now"

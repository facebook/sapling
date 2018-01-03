#!/usr/bin/env bash

set -e

mkdir temp
cd temp

svnadmin create testrepo
svn checkout file://`pwd`/testrepo client

cd client
mkdir trunk
mkdir -p branches
mkdir -p tags

svn add trunk branches tags
svn commit -m "Initial commit"

mkdir trunk/project trunk/other
echo "project trunk" > trunk/project/file
echo "other trunk" > trunk/other/phile
svn add trunk/project trunk/other
svn commit -m "Added file and phile in trunk"

svn up

svn cp trunk tags/tag_from_trunk
svn ci -m 'created tag from trunk'

svn up

svn cp trunk branches/branch
svn ci -m 'created branch from trunk'

svn up

echo "project branch" > branches/branch/project/file
svn ci -m "committed to the project branch"

svn up

echo "trunk2" > trunk/project/file
svn ci -m "committed to trunk again"

svn up

echo "other branch" > branches/branch/other/phile
svn ci -m "committed to the other branch"

svn up

svn cp branches/branch tags/tag_from_branch
svn ci -m "create tag from branch"

cd ..
svnadmin dump testrepo > ../subprojects.svndump

echo "Created subprojects.svndump"
echo "You might want to clean up ${PWD} now"

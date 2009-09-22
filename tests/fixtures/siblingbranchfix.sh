#!/bin/sh

mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
cd wc
mkdir branches trunk tags
svn add *
svn ci -m 'btt'
cd trunk

echo a > a
svn add a
svn ci -m 'Add file.'
svn up

cd ..
svn cp trunk wrongbranch
svn ci -m 'Branch to repo root dir.'
svn up

svn mv wrongbranch branches/wrongbranch
svn ci -m 'Move branch to correct branches location'
svn up

cd ../..
svnadmin dump temp/repo > siblingbranchfix.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in siblingbranchfix.svndump'
exit 0

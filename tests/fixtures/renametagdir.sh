#!/bin/sh
# inspired by Python r62868

mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
export REPO=file://`pwd`/repo
cd wc
mkdir branches trunk tags
svn add *
svn ci -m 'btt'

echo a > trunk/a
svn add trunk/a
svn ci -m 'Add file.'
svn up

svn cp trunk branches/test
svn ci -m 'Branch.'
svn up

cd ..
svn cp -m 'First tag.' $REPO/branches/test@3 $REPO/tags/test-0.1
svn cp -m 'Weird tag.' $REPO/branches/test@3 $REPO/tags/test-0.1/test

cd ..
svnadmin dump temp/repo > renametagdir.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in renametagdir.svndump'
exit 0

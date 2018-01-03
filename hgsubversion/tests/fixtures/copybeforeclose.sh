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
svn cp trunk branches/test
svn ci -m 'Branch.'
svn up

cd branches/test/
svn mv a b
svn ci -m 'Move on branch.'
svn up

cd ../../
svn up
svn rm branches/test
svn ci -m 'Close branch.'

cd ../..
svnadmin dump temp/repo > copybeforeclose.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in copybeforeclose.svndump'
exit 0

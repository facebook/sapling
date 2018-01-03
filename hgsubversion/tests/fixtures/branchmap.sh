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

svn cp trunk branches/badname
svn ci -m 'Branch to be renamed.'
svn up

svn cp trunk branches/feature
svn ci -m 'Branch to be unnamed.'
svn up

cd ../..
svnadmin dump temp/repo > branchmap.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in branchmap.svndump'
exit 0

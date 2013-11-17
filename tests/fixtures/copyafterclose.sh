#!/bin/sh

rm -rf temp
mkdir temp
cd temp
svnadmin create repo
repo=file://`pwd`/repo
svn co $repo wc
cd wc
mkdir branches trunk tags
svn add *
svn ci -m 'btt'

cd trunk
echo trunk1 > file
mkdir dir
echo trunk1 > dir/file
svn add file dir
svn ci -m 'Add file and dir.'
cd ..
svn up

svn cp trunk branches/test
svn ci -m 'Branch.'
svn up

cd branches/test/
echo branch1 > file
echo branch1 > dir/file
svn ci -m 'edit on branch.'
cd ../../
svn up

cd trunk
echo trunk2 > file
echo trunk2 > dir/file
svn ci -m 'edit on trunk'
cd ..
svn up

svn rm trunk
svn ci -m 'Close trunk.'
svn up

cd branches/test
svn rm file
svn cp $repo/trunk/file@5 file
svn rm dir
svn cp $repo/trunk/dir@5 dir
svn ci -m 'copy from trunk before close'
cd ../..
svn up

cd ../..
svnadmin dump temp/repo > copyafterclose.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in copyafterclose.svndump'
exit 0

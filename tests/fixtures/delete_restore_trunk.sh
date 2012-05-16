#!/bin/bash
set -e
mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
cd wc
mkdir branches trunk tags
svn add *
svn ci -m 'btt'
echo foo > trunk/foo
svn add trunk/foo
svn ci -m 'add file'
svn up
svn rm trunk
svn ci -m 'delete trunk'
svn up
cd ..
svn cp -m 'restore trunk' file://`pwd`/repo/trunk@2 file://`pwd`/repo/trunk
cd wc
svn up
echo bar >> trunk/foo
svn ci -m 'append to file'
svn up
cd ../..
svnadmin dump temp/repo > delete_restore_trunk.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in branch_delete_parent_dir.svndump'
exit 0

#!/bin/sh
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
svn cp trunk branches/dev_branch
svn ci -m 'branch'
svn up
svn rm branches
svn ci -m 'delete branches dir'
cd ..
cd ..
svnadmin dump temp/repo > branch_delete_parent_dir.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in branch_delete_parent_dir.svndump'
exit 0

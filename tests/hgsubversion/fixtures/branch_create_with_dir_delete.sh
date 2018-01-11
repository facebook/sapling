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
for a in alpha beta gamma delta ; do
    echo $a > $a
    svn add $a
done
svn ci -m 'Add files.'
mkdir al
echo foo > al/foo
svn add al
svn ci -m 'add directory al to delete on the branch'
cd ..
svn up
svn cp trunk branches/dev_branch
svn rm branches/dev_branch/al
svn ci -m 'branch'
cd branches/dev_branch
svn rm delta
echo narf > alpha
echo iota > iota
svn add iota
svn ci -m 'branch changes'
cd ../../../..
svnadmin dump temp/repo > branch_create_with_dir_delete.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in branch_create_with_dir_delete.svndump'
exit 0

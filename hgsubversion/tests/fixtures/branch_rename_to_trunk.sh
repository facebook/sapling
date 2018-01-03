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
cd ..
svn up
svn cp trunk branches/dev_branch
svn ci -m 'branch'
cd branches/dev_branch
svn rm delta
echo narf > alpha
echo iota > iota
svn add iota
svn ci -m 'branch changes'
cd ../..
svn up
svn mv trunk branches/old_trunk
svn ci -m 'move trunk to a branch'
svn up
svn mv branches/dev_branch trunk
svn ci -m 'move dev to trunk'
cd ..
cd ..
svnadmin dump temp/repo > branch_rename_to_trunk.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in branch_rename_to_trunk.svndump'
exit 0

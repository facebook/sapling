#!/bin/sh
mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc

cd wc
mkdir brances trunk tags
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
svn cp trunk brances/dev_branch
svn ci -m 'branch'

cd brances/dev_branch
svn rm delta
echo narf > alpha
echo iota > iota
svn add iota
svn ci -m 'branch changes'

cd ../..
svn up
svn mv brances branches
svn ci -m 'move branches to branches'

cd ..
cd ..

svnadmin dump temp/repo > rename_branch_parent_dir.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in rename_branch_parent_dir.svndump'
exit 0

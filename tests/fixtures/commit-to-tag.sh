#!/bin/sh
mkdir temp
cd temp
svnadmin create repo
REPOPATH="file://`pwd`/repo"
svn co $REPOPATH wc
cd wc
mkdir -p branches/magic trunk tags
svn add *
svn ci -m 'btt'
cd branches/magic
for a in alpha beta gamma; do
    echo $a > $a
    svn add $a
    svn ci -m "Add file $a"
done
cd ../..
svn up
svn cp $REPOPATH/branches/magic $REPOPATH/tags/will-edit -m 'Make tag to edit'
svn up

cd branches/magic
for a in delta iota lambda; do
    echo $a > $a
    svn add $a
    svn ci -m "Add file $a"
done
cd ../..

cd tags/will-edit
svn rm alpha
svn ci -m 'removed alpha on a tag. Moves tag, implicit branch.'
cd ../..

cd branches/magic
for a in omega; do
    echo $a > $a
    svn add $a
    svn ci -m "Add file $a"
done
cd ../..

cd ../..
svnadmin dump temp/repo > commit-to-tag.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in commit-to-tag.svndump'
exit 0

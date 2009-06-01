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
for a in alpha beta gamma delta iota zeta eta theta ; do
    echo $a > $a
    svn add $a
    svn ci -m "Add file $a"
done
cd ../..
svn up
svn cp $REPOPATH/branches/magic $REPOPATH/tags/magic -m 'Make magic tag'
svn rm $REPOPATH/branches/magic/theta -m 'remove a file'
svn cp $REPOPATH/branches/magic $REPOPATH/tags/magic2 -m 'Tag magic again'

cd ../..
svnadmin dump temp/repo > tag_name_same_as_branch.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in tag_name_same_as_branch.svndump'
exit 0

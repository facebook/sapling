#!/bin/sh
mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
cd wc
mkdir branches trunk tags
mkdir tags/versions
mkdir tags/blah
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
svn cp branches/dev_branch tags/versions/branch_version
svn ci -m 'Make a tag in tags/versions from branches/dev_branch'
svn up
svn cp trunk tags/blah/trunktag
svn ci -m 'Make a tag in tags/blah from trunk'
svn up
cd ../..
svnadmin dump temp/repo > unusual_tags.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in unusual_tags.svndump'
exit 0

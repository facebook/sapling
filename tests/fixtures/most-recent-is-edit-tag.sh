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
svn cp branches/dev_branch tags/some-tag
svn ci -m 'Make a tag.'
svn up
echo foo > tags/some-tag/alpha
svn ci -m 'edit that tag'
cd ../..
svnadmin dump temp/repo > most-recent-is-edit-tag.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in most-recent-is-edit-tag.svndump'
exit 0

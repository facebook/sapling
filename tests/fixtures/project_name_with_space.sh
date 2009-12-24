#!/bin/sh
mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
cd wc
mkdir 'project name'
cd 'project name'
mkdir branches trunk tags
cd ..
svn add *
svn ci -m 'btt'

cd 'project name'/trunk
for a in alpha beta gamma delta ; do
    echo $a > $a
    svn add $a
done
svn ci -m 'Add files.'

mkdir al
echo foo > al/foo
svn add al
svn ci -m 'add directory al'

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

cd ../../../../..
svnadmin dump temp/repo > project_name_with_space.svndump

echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in project_name_with_space.svndump'

exit 0

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

echo 'foo bar' > 'foo bar'
svn add 'foo bar'
svn ci -m 'Add files.'

mkdir 'blah blah'
echo 'another file' > 'blah blah/another file'
svn add 'blah blah'
svn ci -m 'Add files.'

cd ..
svn up
svn cp trunk branches/dev_branch
svn ci -m 'Make a branch'
cd ../..

svnadmin dump temp/repo > spaces-in-path.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in spaces-in-path.svndump'
exit 0

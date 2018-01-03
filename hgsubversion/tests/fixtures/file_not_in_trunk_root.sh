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
mkdir narf
cd narf
for a in alpha beta gamma delta ; do
    echo $a > $a
done
cd ..
svn add narf
svn ci -m 'Add files.'
cd ../../..
svnadmin dump temp/repo > file_not_in_trunk_root.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in file_not_in_trunk_root.svndump'
exit 0

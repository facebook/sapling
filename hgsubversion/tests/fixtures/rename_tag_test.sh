#!/bin/sh

mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
export REPO=file://`pwd`/repo
cd wc
mkdir branches trunk tags
svn add *
svn ci -m 'Empty dirs.'

echo 'file: alpha' > trunk/alpha
svn add trunk/alpha
svn ci -m 'Add alpha'
svn up

echo 'Data of beta' > trunk/beta
svn add trunk/beta
svn ci -m 'Add beta'
svn up
cd ..

svn cp -m 'tagging r3' $REPO/trunk@3 $REPO/tags/tag_r3
svn cp -m 'tag from a tag' $REPO/tags/tag_r3 $REPO/tags/copied_tag
svn mv -m 'rename a tag' $REPO/tags/copied_tag $REPO/tags/other_tag_r3
cd ..
svnadmin dump temp/repo > rename_tag_test.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in renametagdir.svndump'
exit 0

# -*- coding: utf-8 -*-
#!/bin/sh
#
# Use this script to generate encoding.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
mkdir tags
cd ..

svnadmin create svn-repo
svnurl=file://`pwd`/svn-repo
svn import project-orig $svnurl -m "init projA"

svn co $svnurl project
cd project
echo e > trunk/é
mkdir trunk/à
echo d > trunk/à/é
svn add trunk/é trunk/à
svn ci -m hello

# Copy files and directories
svn mv trunk/é trunk/è
svn mv trunk/à trunk/ù
svn ci -m "copy files"

# Remove files
svn rm trunk/è
svn rm trunk/ù
svn ci -m 'remove files'
cd ..

svnadmin dump svn-repo > ../encoding.svndump

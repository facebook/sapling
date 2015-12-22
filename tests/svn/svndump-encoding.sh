#!/bin/sh
# -*- coding: utf-8 -*-
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

# Create branches with and from weird names
svn up
svn cp trunk branches/branché
echo a > branches/branché/a
svn ci -m 'branch to branché'
svn up
svn cp branches/branché branches/branchée
echo a >> branches/branché/a
svn ci -m 'branch to branchée'

# Create tag with weird name
svn up
svn cp trunk tags/branché
svn ci -m 'tag trunk'
svn cp branches/branchée tags/branchée
svn ci -m 'tag branché'
cd ..

svnadmin dump svn-repo > ../encoding.svndump

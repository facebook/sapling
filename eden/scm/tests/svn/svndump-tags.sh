#!/bin/sh
#
# Use this script to generate tags.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
mkdir tags
mkdir unrelated
cd ..

svnadmin create svn-repo
svnurl=file://`pwd`/svn-repo
svn import project-orig $svnurl -m "init projA"

svn co $svnurl project
cd project
echo a > trunk/a
svn add trunk/a
svn ci -m adda
echo a >> trunk/a
svn ci -m changea
echo a >> trunk/a
svn ci -m changea2
# Add an unrelated commit to test that tags are bound to the
# correct "from" revision and not a dummy one
echo a >> unrelated/dummy
svn add unrelated/dummy
svn ci -m unrelatedchange
# Tag current revision
svn up
svn copy trunk tags/trunk.v1
svn copy trunk tags/trunk.badtag
svn ci -m "tagging trunk.v1 trunk.badtag"
echo a >> trunk/a
svn ci -m changea3
# Fix the bad tag
# trunk.badtag should not show in converted tags
svn up
svn mv tags/trunk.badtag tags/trunk.goodtag
svn ci -m "fix trunk.badtag"
echo a >> trunk/a
svn ci -m changea
# Delete goodtag and recreate it, to test we pick the good one
svn rm tags/trunk.goodtag
svn ci -m removegoodtag
svn up
svn copy trunk tags/trunk.goodtag
svn ci -m recreategoodtag
cd ..

svnadmin dump svn-repo > ../tags.svndump
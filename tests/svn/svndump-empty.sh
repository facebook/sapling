#!/bin/sh
#
# Use this script to generate empty.svndump
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
mkdir trunk/dir
echo a > trunk/dir/a
svn add trunk/dir
svn ci -m adddir

echo b > trunk/b
svn add trunk/b
svn ci -m addb

echo c > c
svn add c
svn ci -m addc
cd ..

# svnsync repo/trunk/dir only so the last two revisions are empty
svnadmin create svn-empty
cat > svn-empty/hooks/pre-revprop-change <<EOF
#!/bin/sh
exit 0
EOF
chmod +x svn-empty/hooks/pre-revprop-change
svnsync init --username svnsync file://`pwd`/svn-empty file://`pwd`/svn-repo/trunk/dir
svnsync sync file://`pwd`/svn-empty
svn log -v file://`pwd`/svn-empty

svnadmin dump svn-empty > ../empty.svndump

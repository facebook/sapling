#!/bin/sh
#
# Create emptyrepo2.svndump
#
# The generated repository contains a sequence of empty revisions
# created with a combination of svnsync and filtering

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir -p sub/trunk other
echo a > other/a
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m init

svn co $svnurl project
cd project
echo a >> other/a
svn ci -m othera
echo a >> other/a
svn ci -m othera2
echo b > sub/trunk/a
svn add sub/trunk/a
svn ci -m adda
cd ..

svnadmin create testrepo2
cat > testrepo2/hooks/pre-revprop-change <<EOF
#!/bin/sh
exit 0
EOF
chmod +x testrepo2/hooks/pre-revprop-change

svnurl2=file://`pwd`/testrepo2
svnsync init --username svnsync $svnurl2 $svnurl/sub
svnsync sync $svnurl2

svnadmin dump testrepo2 > ../emptyrepo2.svndump


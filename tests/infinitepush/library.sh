#!/bin/bash

scratchnodes() {
  for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
     echo ${node##*/} `cat $node`
  done
}

scratchbookmarks() {
  for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
     echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  done
}

setupcommon() {
  cat >> $HGRCPATH << EOF
[extensions]
commitcloud=
infinitepush=
pullcreatemarkers=
[ui]
ssh = python "$TESTDIR/dummyssh"
[infinitepush]
branchpattern=re:scratch/.*
bgssh = python "$TESTDIR/dummyssh" -bgssh
EOF
}

setupserver() {
cat >> .hg/hgrc << EOF
[infinitepush]
server=yes
indextype=disk
storetype=disk
reponame=babar
EOF
}

setupsqlclienthgrc() {
cat << EOF > .hg/hgrc
[ui]
ssh=python "$TESTDIR/dummyssh"
[extensions]
infinitepush=
[infinitepush]
branchpattern=re:scratch/.+
server=False
[paths]
default = ssh://user@dummy/server
EOF
}

setupsqlserverhgrc() {
cat << EOF > .hg/hgrc
[ui]
ssh=python "$TESTDIR/dummyssh"
[extensions]
infinitepush=
[infinitepush]
branchpattern=re:scratch/.+
server=True
indextype=sql
storetype=disk
reponame=$1
EOF
}

enableforwardfill() {
cat << EOF >> .hg/hgrc
[infinitepush]
forwardfill=True
EOF
}

createdb() {
mysql -h $DBHOST -P $DBPORT -u $DBUSER $DBPASSOPT -e "CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT <<EOF
DROP TABLE IF EXISTS nodestobundle;
DROP TABLE IF EXISTS bookmarkstonode;
DROP TABLE IF EXISTS bundles;
DROP TABLE IF EXISTS nodesmetadata;
DROP TABLE IF EXISTS forwardfillerqueue;
$(cat $TESTDIR/infinitepush/schema.sql)
EOF
}

querysqlindex() {
  mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" -D "$DBNAME" "$DBPASSOPT" -e "$1" 2>/dev/null
}

setupdb() {
source "$TESTDIR/hgsql/library.sh"
echo "sqlhost=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS" >> .hg/hgrc

createdb
}

waitbgbackup() {
  sleep 1
  hg debugwaitbackup
}

mkcommitautobackup() {
    echo $1 > $1
    hg add $1
    hg ci -m $1 --config infinitepushbackup.autobackup=True
}

setuplogdir() {
  mkdir $TESTTMP/logs
  chmod 0755 $TESTTMP/logs
  chmod +t $TESTTMP/logs
}

debugsshcall() {
  sed -n '/^running .*dummyssh.*$/p'
}

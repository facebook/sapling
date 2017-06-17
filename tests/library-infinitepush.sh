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
infinitepush=$TESTDIR/../infinitepush
[ui]
ssh = python "$TESTDIR/dummyssh"
[infinitepush]
branchpattern=re:scratch/.*
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
infinitepush=$TESTDIR/../infinitepush
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
infinitepush=$TESTDIR/../infinitepush
[infinitepush]
branchpattern=re:scratch/.+
server=True
indextype=sql
storetype=disk
reponame=$1
EOF
}

createdb() {
mysql -h $DBHOST -P $DBPORT -u $DBUSER -p"$DBPASS" -e "CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" <<EOF
DROP TABLE IF EXISTS nodestobundle;
DROP TABLE IF EXISTS bookmarkstonode;
DROP TABLE IF EXISTS bundles;
DROP TABLE IF EXISTS nodesmetadata;
$(cat $TESTDIR/../infinitepush/schema.sql)
EOF
}

setupdb() {
[[ -e $TESTDIR/getdb.sh ]] || { echo 'skipped: missing getdb.sh'; exit 80; }
source $TESTDIR/getdb.sh >/dev/null

if [[ -z $DBHOST && -z $DBPORT && -n $DBHOSTPORT ]]; then
    # Assuming they are set using the legacy way: $DBHOSTPORT
    DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
    DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
fi

[[ -z $DBHOST ]] && DBHOST=localhost
[[ -z $DBPORT ]] && DBPORT=3306
[[ -z $DBPASS && -n $PASSWORD ]] && DBPASS="$PASSWORD"
[[ -z $DBUSER && -n $USER ]] && DBUSER="$USER"
[[ -z $DBNAME ]] && DBNAME="testdb_hg_$$_$TIME"

echo "sqlhost=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS" >> .hg/hgrc

createdb
}

waitbgbackup() {
  sleep 1
  hg debugwaitbackup
}

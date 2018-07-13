${PYTHON:-python} -c 'import lz4' || exit 80

CACHEDIR=$PWD/hgcache
cat >> $HGRCPATH <<EOF
[remotefilelog]
cachepath=$CACHEDIR
debug=True
historypackv1=True
datapackversion=1
[extensions]
remotefilelog=
rebase=
mq=
[ui]
ssh=python "$TESTDIR/dummyssh"
[server]
preferuncompressed=True
[experimental]
changegroup3=True
[rebase]
singletransaction=True
EOF

hgcloneshallow() {
  local name
  local dest
  orig=$1
  shift
  dest=$1
  shift
  hg clone --shallow --config remotefilelog.reponame=master $orig $dest $@
  cat >> $dest/.hg/hgrc <<EOF
[remotefilelog]
reponame=master
datapackversion=1
[phases]
publish=False
EOF
}

hgcloneshallowlfs() {
  local name
  local dest
  local lfsdir
  orig=$1
  shift
  dest=$1
  shift
  lfsdir=$1
  shift
  hg clone --shallow --config "extensions.lfs=" --config "lfs.url=$lfsdir" --config remotefilelog.reponame=master $orig $dest $@
  cat >> $dest/.hg/hgrc <<EOF
[extensions]
lfs=
[lfs]
url=$lfsdir
[remotefilelog]
reponame=master
datapackversion=1
[phases]
publish=False
EOF
}

hginit() {
  local name
  name=$1
  shift
  hg init $name $@ --config remotefilelog.reponame=master
}

clearcache() {
  rm -rf $CACHEDIR/*
}

mkcommit() {
  echo "$1" > "$1"
  hg add "$1"
  hg ci -m "$1"
}

ls_l() {
  $PYTHON $TESTDIR/ls-l.py "$@"
}

getmysqldb() {
if ! ${PYTHON:-python} -c "import mysql.connector" 2>/dev/null; then
  echo "skipped: mysql-connector-python missing"
  exit 80
fi

GETDB_PATH="$TESTDIR/${HGTEST_GETDB_PATH:-getdb.sh}"

if [[ ! -f "$GETDB_PATH" ]]; then
  echo "skipped: getdb.sh missing"
  exit 80
fi

# shellcheck source=/dev/null
source "$GETDB_PATH" >/dev/null

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
if [[ -z $DBPASS ]]; then
    DBPASSOPT=''
else
    DBPASSOPT='-p'"$DBPASS"
fi
}

createpushrebaserecordingdb() {
mysql -h $DBHOST -P $DBPORT -u $DBUSER $DBPASSOPT -e "CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT <<EOF
DROP TABLE IF EXISTS pushrebaserecording;
$(cat $TESTDIR/pushrebase_replay_schema.sql)
EOF
}

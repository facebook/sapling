cat >> $HGRCPATH<<EOF
[extensions]
p4fastimport=
EOF

# create p4 depot
p4wd="$TESTTMP/p4"
hgwd="$TESTTMP/hg"
P4ROOT="$TESTTMP/depot"; export P4ROOT
P4AUDIT="$P4ROOT/audit"; export P4AUDIT
P4JOURNAL="$P4ROOT/journal"; export P4JOURNAL
P4LOG="$P4ROOT/log"; export P4LOG
P4PORT=localhost:$HGPORT; export P4PORT
P4DEBUG=1; export P4DEBUG

mkdir "$hgwd"
mkdir "$p4wd"
cd "$p4wd"

# start the p4 server
[ ! -d $P4ROOT ] && mkdir $P4ROOT
p4d $P4DOPTS -f -J off >$P4ROOT/stdout 2>$P4ROOT/stderr &
echo $! >> $DAEMON_PIDS
trap "echo stopping the p4 server ; p4 admin stop" EXIT

# wait for the server to initialize
while ! p4 ; do
   sleep 1
done >/dev/null 2>/dev/null

# create a client spec
cd $p4wd
P4CLIENT=hg-p4-import; export P4CLIENT
DEPOTPATH=${DEPOTPATH:-//depot/...}
p4 client -o | sed '/^View:/,$ d' >p4client
echo View: >>p4client
echo " $DEPOTPATH //$P4CLIENT/..." >>p4client
p4 client -i <p4client >/dev/null

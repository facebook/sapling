/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Default config. Partially migrated from configitems.py.
///
/// Lowest priority. Should always be loaded.
pub static CONFIG: StaticConfig = static_config!("builtin:core" => r#"
[treestate]
mingcage=900
minrepackthreshold=10M
repackfactor=3

[ui]
timeout=600
color=auto
paginate=true
ignorerevnum=True

[checkout]
resumable=true

[tracing]
stderr=false
threshold=10

[format]
generaldelta=false
usegeneraldelta=true

[color]
status.added=green bold
status.clean=none
status.copied=none
status.deleted=cyan bold underline
status.ignored=black bold
status.modified=blue bold
status.removed=red bold
status.unknown=magenta bold underline

[commands]
naked-default.in-repo=sl
naked-default.no-repo=help

[git]
filter=blob:none

[unsafe]
filtersuspectsymlink=true

[experimental]
exportstack-max-bytes=1M
allow-non-interactive-editor=true

log-implicit-follow-threshold=10000

titles-namespace=true
local-committemplate=true

evalframe-passthrough=true

run-python-hooks-via-pyhook=true

lock-free-pull=true

[zsh]
completion-age=7
completion-description=false

[merge]
enable-merge-tool-script=true

[remotenames]
autocleanupthreshold=50
selectivepulldefault=master
selectivepulldiscovery=true
autopullhoistpattern=
autopullpattern=re:^(?:default|remote)/[A-Za-z0-9._/-]+$
hoist=default

[scmstore]
handle-tree-parents=true

[filetype-patterns]
**/BUCK=buck
**.bzl=buck
**.php=hack
**.cpp=cpp
**.c=c
**.m=object-c
**.h=dot-h
**.py=python
**.js=javascript
**.ts=typescript
**.java=java
**.kt=kotlin
**.rs=rust
**.cs=csharp

[automerge]
merge-algos=adjacent-changes,subset-changes
mode=accept
import-pattern:buck=re:^\s*(".*//.*",|load\(.*)$
import-pattern:hack=re:^\s*use .*$
import-pattern:cpp=re:^\s*#include .*$
import-pattern:c=re:^\s*#include .*$
import-pattern:object-c=re:^\s*(#include|#import) .*$
import-pattern:dot-h=re:^\s*(#include|#import) .*$
import-pattern:python=re:^\s*import .*$
import-pattern:javascript=re:^\s*import .*$
import-pattern:typescript=re:^\s*import .*$
import-pattern:java=re:^\s*import .*$
import-pattern:kotlin=re:^\s*import .*$
import-pattern:rust=re:^\s*use .*$
import-pattern:csharp=re:^\s*using .*$
import-pattern:go=re:^\s*using .*$

[clone]
use-commit-graph=true

[pager]
stderr=true

[blackbox]
maxsize=100 MB
maxfiles=3

[bundle2]
rechunkthreshold=1MB

[bundle]
reorder=auto

[chgserver]
idletimeout=3600

[commands]
update.check=noconflict

[copytrace]
sourcecommitlimit=100
enableamendcopytrace=True
amendcopytracecommitlimit=100

[debug]
dirstate.delaywrite=0

[devel]
legacy.revnum=accept
strip-obsmarkers=True

[discovery]
full-sample-size=200
initial-sample-size=100

[doctor]
check-lag-name=master
check-lag-threshold=50
check-too-many-names-threshold=20

[edenfs]
tree-fetch-depth=3

[email]
method=smtp

[experimental]
bundle2-advertise=True
disable-narrow-heads-ssh-server=True
mmapindexthreshold=1
format.compression=zlib
graph.renderer=lines
narrow-heads=True
pathhistory.find-merge-conflicts=True
revf64compat=True
uncommitondirtywdir=True
rebaseskipobsolete=True

[format]
cgdeltabase=default
dirstate=2
usegeneraldelta=True

[fsmonitor]
warn_when_unused=True
warn_update_file_count=50000

[git]
submodules=True

[gpg]
enabled=True

[histgrep]
allowfullrepogrep=True

[log]
simplify-grandparents=True

[merge]
checkunknown=abort
checkignored=abort
followcopies=True
on-failure=continue
preferancestor=*

[metalog]
track-config=True

[mononokepeer]
sockettimeout=15.0

[mutation]
enabled=True
record=True

[pager]
pager=internal:streampager

[patch]
eol=strict
fuzz=2

[phases]
new-commit=draft
publish=True

[profiling]
format=text
freq=1000
limit=30
minelapsed=0
nested=0
showmax=0.999
sort=inlinetime
statformat=hotpath
type=stat

[progress]
changedelay=1
clear-complete=True
delay=3
estimateinterval=10.0
format=topic, bar, number, estimate
refresh=0.1
renderer=classic

[pull]
automigrate=True
buffer-commit-count=100000
httpbookmarks=True
httpmutation=True
master-fastpath=True

[exchange]
httpcommitlookup=True

[push]
pushvars.server=True
requirereasonmsg=

[sendunbundlereplay]
respondlightly=True

[server]
bookmarks-pushkey-compat=True
bundle1=True
compressionengines=<class 'list'>
maxhttpheaderlen=1024
uncompressed=True
zliblevel=-1

[smallcommitmetadata]
entrylimit=100

[smtp]
tls=none

[ui]
allowmerge=True
archivemeta=True
autopullcommits=True
changesetdate=authordate
clonebundles=True
debugger=ipdb
exitcodemask=255
fancy-traceback=True
git=git
gitignore=True
mergemarkers=basic
mergemarkertemplate={node|short} {ifeq(tags, "tip", "", ifeq(tags, "", "", "{tags} "))}{if(bookmarks, "{bookmarks} ")}{ifeq(branch, "default", "", "{branch} ")}- {author|user}: {desc|firstline}
portablefilenames=warn
remotecmd=hg
ssh=ssh
style=
textwidth=78
version-age-threshold-days=31
enableincomingoutgoing=True

[visibility]
enabled=True

[worker]
# Windows defaults to a limit of 512 open files. A buffer of 128
# should give us enough headway.
backgroundclosemaxqueue=384
backgroundcloseminfilecount=2048
backgroundclosethreadcount=4
enabled=True

[sampling]
filepath=

[progress]
statefile=

[smartlog]
collapse-obsolete=True
max-commit-threshold=1000

[commitcloud]
servicetype=remote
scm_daemon_tcp_port=15432
enablestatus=True
enableprogress=True
pullsizelimit=300

[infinitepushbackup]
enablestatus=True
maxheadstobackup=-1

[infinitepush]
httpbookmarks=True

[grep]
command=xargs -0 grep

[globalrevs]
onlypushrebase=True
startrev=0

[fbscmquery]
auto-username=true

[phrevset]
autopull=True
graphqlonly=True
abort-if-git-diff-unavailable=True

[fbcodereview]
hide-landed-commits=True

[remotenames]
bookmarks=True
calculatedistance=True
precachecurrent=True
precachedistance=True
resolvenodes=True
tracking=True
racy-pull-on-push=True

[blackbox]
track=*

[treemanifest]
fetchdepth=65536
stickypushpath=True
http=True

[sigtrace]
pathformat=/tmp/trace-%(pid)s-%(time)s.log
signal=USR1
mempathformat=/tmp/memtrace-%(pid)s-%(time)s.log
memsignal=USR2
interval=0

[errorredirect]
fancy-traceback=True

[remotefilelog]
updatesharedcache=True
getpackversion=1
http=True

[histedit]
linelen=80

[absorb]
maxdescwidth=50

[fsmonitor]
mode=on
timeout=10
dirstate-nonnormal-file-threshold=200
watchman-changed-file-threshold=200
fallback-on-watchman-exception=True
tcp-host=::1
tcp-port=12300
wait-full-crawl=True

[commands]
amend.autorebase=True

[update]
nextpreferdraft=True

[github]
pull-request-include-reviewstack=True

[shelve]
maxbackups=10

[preventpremegarepoupdates]
message=Checking out commits from before megarepo merge is discouraged. The resulting checkout will contain just the contents of one git subrepo. Many tools might not work as expected. Do you want to continue (Yn)?  $$ &Yes $$ &No
dangerrevset=not(contains('.megarepo/remapping_state'))
"#);

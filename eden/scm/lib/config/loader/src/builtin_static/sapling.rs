/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use staticconfig::static_config;
use staticconfig::StaticConfig;

/// Config used by the Sapling identity. Might be merged with core config.
pub static CONFIG: StaticConfig = static_config!("builtin:sapling" => r###"
[alias]
metaedit=metaedit --batch
journal=journal --verbose
restack=rebase --restack
restack:doc=automatically restack commits
 ""
 "    When commits are modified by commands like amend and absorb, their"
 "    descendant commits may be left behind as orphans.  Rebase these"
 "    orphaned commits onto the newest versions of their ancestors, making"
 "    the stack linear again."
sl=smartlog -T '{sl}'
ssl=smartlog -T '{ssl}'

[annotate]
default-flags=user short-date

[automv]
similarity=75

[blackbox]
maxfiles=30
maxsize=5242880
logsource=true
track=command, command_alias, command_finish, command_exception,
 commitcloud_sync, exthook, pythonhook, fsmonitor, watchman, merge_resolve,
 profile, hgsql, sqllock, pushrebase, status, metrics, visibility, perftrace

[color]
log.changeset=bold blue
sl.active=yellow
sl.amended=brightblack:none
sl.branch=bold
sl.book=green
sl.changed=brightblack:none
sl.current=magenta
sl.diff=bold
sl.draft=brightyellow bold
sl.folded=brightblack:none
sl.hiddenlabel=brightblack:none
sl.hiddennode=brightblack:none
sl.histedited=brightblack:none
sl.landed=brightblack:none
sl.undone=brightblack:none
sl.oldbook=bold brightred
sl.public=yellow
sl.rebased=brightblack:none
sl.remote=green
sl.snapshotnode=brightblue:blue
sl.split=brightblack:none
sl.tasks=bold
sl.backuppending=cyan
sl.backupfail=red
sl.shelvedlabel=bold
ssl.abandoned=brightblack:black+bold
ssl.accepted=brightgreen bold
ssl.committed=cyan
ssl.landing=green
ssl.review=#f7923b:color214:brightyellow bold
ssl.revision=#f21395:color199:brightmagenta bold
ssl.unpublished=#8d949e:color248:none
ssl.landfailed=red
ssl.finalreview=#8036cc:color92:magenta
ssl.unsync=brightred bold
ssl.signal_okay=green
ssl.signal_in_progress=cyan
ssl.signal_warning=yellow
ssl.signal_failed=red
sb.active=green
doctor.treatment=yellow
use-rust=false

[connectionpool]
lifetime=300

[debugnetwork]
speed-test-download-size=10M
speed-test-upload-size=2M

[diff]
git=True
nobinary=True

[directaccess]
loadsafter=tweakdefaults

[experimental]
bundle2-exp=True
changegroup3=True
copytrace=off
crecord=True
disallowhgignorefileset=True
evolution=obsolete
fsmonitor.transaction_notify=True
graphstyle.grandparent=|
histedit.autoverb=True
metalog=True
narrow-heads=True
new-clone-path=True
rebase.multidest=True
samplestatus=3
updatecheck=noconflict
verifyhiddencache=False
worddiff=True
mmapindexthreshold=1
numworkersremover=1
graph.renderer=lines
nativecheckout=True
numworkerswriter=2
network-doctor=True

[extensions]
absorb=
amend=
automv=
blackbox=
chistedit=
clindex=
conflictinfo=
copytrace=
debugnetwork=
dialect=
directaccess=
dirsync=
errorredirect=!
extorder=
fastpartialmatch=
fbhistedit=
fsmonitor=
ghstack=
githelp=
github=
gitrevset=!
hgsubversion=!
histedit=
infinitepush=!
journal=
logginghelper=
lz4revlog=
morestatus=
myparent=
obsshelve=
patchrmdir=!
prmarker=
progressfile=
pullcreatemarkers=
pushrebase=!
rage=!
rebase=
remotefilelog=
remotenames=
schemes=
shelve=
smartlog=
sparse=
strip=
traceprof=
treedirstate=
treemanifest=
tweakdefaults=
undo=

[extorder]
fsmonitor=sqldirstate, sparse
journal=eden

[format]
aggressivemergedeltas=True
generaldelta=True
manifestcachesize=10
maxchainlen=30000
use-zstore-commit-data-revlog-fallback=true
use-zstore-commit-data=False
noloosefile=True
userustmutablestore=True
use-symlink-atomic-write=False

[fsmonitor]
mode=on
timeout=600
track-ignore-files=False
walk_on_invalidate=False
warn-fresh-instance=True
# TODO: T130638905 Update this
sockpath=/opt/facebook/watchman/var/run/watchman/%i-state/sock

[histgrep]
allowfullrepogrep=False

[merge]
checkignored=ignore
checkunknown=ignore
on-failure=continue
printcandidatecommmits=True

[merge-tools]
editmerge.args=$output
editmerge.check=changed
editmerge.premerge=keep
editmergeps.args=$output
editmergeps.check=changed
editmergeps.premerge=keep

[mutation]
automigrate=true
enabled=true
record=true

[phases]
publish=False

[remotefilelog]
cachelimit=20GB
cleanoldpacks=True
fastdatapack=True
historypackv1=True
localdatarepack=True
useruststore=True
manifestlimit=4GB
http=True
userustrepack=True
cachepath=~/.sl_cache
retryprefetch=True
fetchpacks=True
getpackversion=2
write-hgcache-to-indexedlog=True
write-local-to-indexedlog=True

[remotenames]
# TODO what's the right oss value for this?
autopullhoistpattern=
autopullpattern=re:^remote/[A-Za-z0-9._/-]+$
cachedistance=False
disallowedbookmarks=master
 remote/master
 main
 remote/main
disallowedhint=please don't specify 'remote/' prefix in remote bookmark's name
disallowedto=^remote/
hoist=remote
precachecurrent=False
precachedistance=False
pushanonheads=False
pushrev=.
rename.default=remote
resolvenodes=False
selectivepull=True
selectivepulldefault=master
astheaddiscovery=False

[server]
preferuncompressed=True
uncompressed=True

[sigtrace]
interval=60

[templatealias]
sl_hash_minlen=9
sl_phase_label="{ifeq(phase, 'public', 'sl.public', 'sl.draft')}"
sl_node="{label(sl_phase_label, shortest(node, sl_hash_minlen))}"
sl_node_debug="{label(sl_phase_label, '{node}')}"
sl_phase_debug="{ifeq(phase, 'draft', '', '({phase})')}"
sl_undonode="{label('sl.oldbook', '{shortest(node, sl_hash_minlen)}')}"
sl_donode="{label('sl.book', '{shortest(node, sl_hash_minlen)}')}"
sl_some="{label('sl.draft', '{shortest(node, sl_hash_minlen)}')}"
undo_node_info="{if(undonecommits(UNDOINDEX), sl_undonode, if(donecommits(UNDOINDEX), if(revset('{node} and olddraft(0)'), sl_some, sl_donode), sl_node))}"
sl_user="{label('sl.user', author|emailuser)}"
sl_active="{if(activebookmark, label('sl.active', '{activebookmark}*'))}"
sl_labeled_bm="{label('sl.book', bookmark)}"
sl_nonactive="{ifeq(bookmark, active, '', '{sl_labeled_bm} ')}"
sl_others="{strip(bookmarks % '{sl_nonactive}')}"
sl_remote="{label('sl.remote', remotebookmarks)}"
sl_books="{separate(' ', sl_active, sl_others, sl_remote)}"
sl_oldbm="{label('sl.book', oldbookmarks(UNDOINDEX))}"
sl_leaving="{label('sl.oldbook', removedbookmarks(UNDOINDEX))}"
sl_bookchanges="{separate(' ', sl_leaving, sl_oldbm)}"
sl_signal_okay="✓"
sl_signal_failed="✗"
sl_signal_warning="‼"
sl_signal_in_progress="⋯"
github_sl_difflink="{
  if(github_pull_request_url,
    hyperlink(github_pull_request_url, '#{github_pull_request_number}'),
    if(sapling_pr_follower, label('ssl.unpublished', 'follower'))
  )}"
phab_sl_difflink="{hyperlink(separate('', 'https://our.intern.facebook.com/intern/diff/', phabdiff, '/'), phabdiff)}"
sl_difflink="{if(github_repo, github_sl_difflink, phab_sl_difflink)}"
github_sl_diffsignal="{case(github_pull_request_status_check_rollup, 'SUCCESS', sl_signal_okay, 'PENDING', sl_signal_in_progress, 'FAILURE', sl_signal_failed)}"
github_sl_diffsignallabel="{case(github_pull_request_status_check_rollup, 'SUCCESS', 'ssl.signal_okay', 'PENDING', 'ssl.signal_in_progress', 'FAILURE', 'ssl.signal_failed')}"
phab_sl_diffsignal="{case(phabsignalstatus, 'Okay', sl_signal_okay, 'In Progress', sl_signal_in_progress, 'Warning', sl_signal_warning, 'Failed', sl_signal_failed)}"
phab_sl_diffsignallabel="{case(phabsignalstatus, 'Okay', 'ssl.signal_okay', 'In Progress', 'ssl.signal_in_progress', 'Warning', 'ssl.signal_warning', 'Failed', 'ssl.signal_failed')}"
sl_diffsignal="{if(github_repo, github_sl_diffsignal, phab_sl_diffsignal)}"
sl_diffsignallabel="{if(github_repo, github_sl_diffsignallabel, phab_sl_diffsignallabel)}"
sl_diffstatus="{if(github_repo, github_sl_diffstatus, phab_sl_diffstatus)}"
sl_difflabel="{if(github_repo, github_sl_difflabel, phab_sl_difflabel)}"

# When presenting the PR state to the user, if it is Closed/Merged, we use
# that as the state; otherwise (if it is Open/Unknown), we consider the review
# decision to be the PR state.
github_pr_state="{case(github_pull_request_state,
    'CLOSED', 'CLOSED',
    'MERGED', 'MERGED',
    if(github_pull_request_review_decision, github_pull_request_review_decision, 'NO_DECISION')
    )}"
github_sl_diffstatus="{case(github_pr_state,
    'CLOSED', 'Closed',
    'MERGED', 'Merged',
    'APPROVED', 'Approved',
    'CHANGES_REQUESTED', 'Changes Requested',
    'REVIEW_REQUIRED', 'Review Required',
    'NO_DECISION', 'Unreviewed'
    )}"
# At least for ghstack, PRs are never merged directly because of the branch
# tricks it does, so Merged often means Closed, so we use the same style for
# both. We could consider checking for the "Merged" tag to distinguish between
# "closed but merged" and ordinary "closed", which is more like Abandoned in
# Phabricator.
github_sl_difflabel="{case(github_pr_state,
    'CLOSED', 'ssl.committed',
    'MERGED', 'ssl.committed',
    'APPROVED', 'ssl.accepted',
    'CHANGES_REQUESTED', 'ssl.revision',
    'REVIEW_REQUIRED', 'ssl.review',
    'NO_DECISION', 'ssl.unpublished',
    'sl.diff'
    )}"

phab_sl_diffstatus="{phabstatus}{case(phabstatus, 'Waiting For Author', '*', 'Needs Revision', '*')}"
phab_sl_difflabel="{case(phabstatus,
 'Landing', 'ssl.landing',
 'Accepted', 'ssl.accepted',
 'Waiting For Author', 'ssl.revision',
 'Needs Revision', 'ssl.revision',
 'Changes Planned', 'ssl.revision',
 'Committed', 'ssl.committed',
 'Needs Review', 'ssl.review',
 'Abandoned', 'ssl.abandoned',
 'Unpublished', 'ssl.unpublished',
 'Committing', 'ssl.landing',
 'Recently Failed to Land', 'ssl.landfailed',
 'Needs Final Review', 'ssl.finalreview',
 'sl.diff'
 )}"
sl_date="{label('sl.date', '{smartdate(date, sl_date_threshold, age(date), simpledate(date, sl_date_timezone))}')}"
sl_date_threshold=5400
sl_diff="{label('sl.diff', sl_difflink)}"
sl_task="{label('sl.tasks', 'T{task}')}"
sl_tasks="{join(tasks % '{sl_task}', ' ')}"
sl_backupstatus="{ifcontains(rev, revset('notbackedup()'), if(backingup, label('sl.backuppending', '(Backing up)'), ifcontains(rev, revset('{node} and age(\'<10m\')'), label('sl.backuppending', '(Backup pending)'), label('sl.backupfail', '(Not backed up)'))))}"
sl_landed="{if(singlepublicsuccessor, label('sl.landed', '(Landed as {shortest(singlepublicsuccessor, sl_hash_minlen)})'))}"
sl_amended="{if(amendsuccessors, label('sl.amended', '(Amended as {join(amendsuccessors % \'{shortest(amendsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_rebased="{if(rebasesuccessors, label('sl.rebased', '(Rebased as {join(rebasesuccessors % \'{shortest(rebasesuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_split="{if(splitsuccessors, label('sl.split', '(Split into {join(splitsuccessors % \'{shortest(splitsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_folded="{if(foldsuccessors, label('sl.folded', '(Folded into {join(foldsuccessors % \'{shortest(foldsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_histedited="{if(histeditsuccessors, label('sl.histedited', '(Histedited as {join(histeditsuccessors % \'{shortest(histeditsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_undone="{if(undosuccessors, label('sl.undone', '(Undone to {join(undosuccessors % \'{shortest(undosuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_mutation_names=dict(amend="Amended as", rebase="Rebased to", split="Split into", fold="Folded into", histedit="Histedited to", land="Landed as", pushrebase="Landed as")
sl_mutations="{join(mutations % '[{get(sl_mutation_names, operation, \'Rewritten into\')} {join(successors % \'{node|short}\', \', \')}]', ' ')}"
sl_hidden="{ifcontains(rev, revset('hidden()'), label('sl.hiddenlabel', '(hidden)'))}"
sl_label="{ifeq(graphnode, '@', 'sl.current', ifeq(graphnode, 'x', 'sl.hiddennode', ifeq(graphnode, 's', 'sl.snapshotnode')))}"
sl_desclabel="{ifeq(graphnode, '@', 'sl.desc.current', ifeq(graphnode, 'x', 'sl.desc.hiddennode', ifeq(phase, 'public', 'sl.desc.public', 'sl.desc')))}"
sl_shelved="{if(shelveenabled, ifcontains(rev, revset('shelved()'), label('sl.shelvedlabel', '(shelved)')), '')}"
ssl_unsync="{label('ssl.unsync', ifeq(syncstatus, 'unsync', '(local changes)'))}"
sl_stablecommit="{label('sl.stablecommit', smallcommitmeta('arcpull_stable'))}"
sl_node_info="{separate(' ', sl_node, sl_mutations, sl_backup, sl_shelved)}"
sl_node_info_debug="{separate(' ', sl_node_debug, sl_mutations, sl_hidden, sl_backup, sl_shelved, sl_phase_debug)}"
sl_diff_super="{ifeq(graphnode, 's', '', if(sl_diff, separate(' ', label(sl_difflabel, separate(' ', sl_difflink, sl_diffstatus)), label(sl_diffsignallabel, sl_diffsignal), ssl_unsync)))}"
sl_diff_colors="{if(sl_diff, label(sl_difflabel, '{phabdiff}'))}"
sl_header_normal="{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_super="{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff_super, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_colors="{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff_colors, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_debug="{separate('  ', sl_userdefined_prefix, sl_node_info_debug, sl_date, sl_user, sl_diff, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_short="{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_books, sl_userdefined_suffix)}"
sl_desc="{label(sl_desclabel, truncatelonglines(desc|firstline, termwidth - graphwidth - 2, ellipsis))}"
ellipsis='…'
sl_use_short_header="{ifeq(verbosity, 'verbose', '', ifeq(graphnode, '@', '', ifeq(phase, 'public', ifeq('{username|email}', '{author|email}', '', 'true'))))}"
sl="{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_normal, sl_desc, '\n')))}"
ssl="{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_super, sl_desc, '\n')))}"
csl="{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_colors, sl_desc, '\n')))}"
sl_debug="{label(sl_label, separate('\n', sl_header_debug, sl_desc, '\n'))}"
undo_newwp="{if(oldworkingcopyparent(UNDOINDEX), '(working copy will move here)')}"
undopreview="{separate('\n', separate('  ', undo_node_info, undo_newwp, sl_user, sl_diff, sl_tasks, sl_bookchanges), '{sl_desc}', '\n')}"
sb_date="{date(date, '%x')}"
sb_item="{sb_date} {bookmarks}\n           {desc|firstline}\n"
sb_active="{label('sb.active', sb_item)}"
sb="{if(activebookmark, sb_active, sb_item)}"
sl_cloud_node="{label(sl_phase_label, truncatelonglines(node, sl_hash_minlen))}"
sl_cloud_header_super="{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_user, sl_diff_super, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_cloud_header_normal="{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_user, sl_diff, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_cloud_header_short="{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_books, sl_userdefined_suffix)}"
sl_cloud="{label(sl_label, if(sl_use_short_header, sl_cloud_header_short, separate('\n', sl_cloud_header_normal, sl_desc, '\n')))}"
ssl_cloud="{label(sl_label, if(sl_use_short_header, sl_cloud_header_short, separate('\n', sl_cloud_header_super, sl_desc, '\n')))}"
jf_submit_template='\{
 "node": {node|json},
 "date": {date|json},
 "desc": {desc|utf8|json},
 "diff": {diff()|json},
 "files": {files|json},
 "user": {author|utf8|json},
 "parents": ["{p1node}" {ifeq(p2node, "0000000000000000000000000000000000000000","",", \"{p2node}\"")}],
 "publicbase": {singlepublicbase|json},
 "publicbase_svnrev": {revset('last(::%d - not public())', rev) % '{svnrev}'|json},
 "parentrevision": {myparentdiff|json},
 "phase": {phase|json}
 },\n'
sl_backup="{if(enabled('commitcloud'),sl_backupstatus)}"

[ui]
style=sl_default

[revsetalias]
sb(n)=first(sort(bookmark(), -rev), n)
sba=sort(bookmark(), -rev)
top=heads(. ::)
bottom=first(draft() & ::.)
base=last(public() & ::.)
obsrelated(x)=mutrelated(x)
focusedsmartlog(x)=focusedbranch(x) + draftbranch(x)^ + present(master)
stable=getstablerev()
stable_for($1)=getstablerev($1)

[treestate]
automigrate=True

[treemanifest]
sendtrees=True
treeonly=True
http=True
usecunionstore=False
rustmanifest=True
pullprefetchrevs=master
ondemandfetch=True
prefetchdraftparents=False
useruststore=True

[tweakdefaults]
showupdated=True
defaultdest=remote/master
tagsmessage=NOTE: new tags are disabled in this repository
singlecolonwarn=True
singlecolonmsg=':' is deprecated; use '::' instead.

[ui]
disallowemptyupdate=True
enableincomingoutgoing=False
hyperlink=True
interface=curses
logmeasuredtimes=True
merge:interactive=editmerge
mergemarkers=detailed
origbackuppath=.sl/origbackups
rollback=False
suggesthgprev=True
allowmerge=True
disallowedbrancheshint=use bookmarks instead
threaded=False
merge=internal:merge

[progress]
renderer=rust:simple

[verify]
skipmanifests=True

[visibility]
automigrate=start
enabled=true

[worker]
rustworkers=True
numcpus=4

[clindex]
nodemap=True
verify=False

[committemplate]
changeset={if(desc, desc, emptymsg)}\n
 SL: Enter commit message.  Lines beginning with 'SL:' are removed.
 SL: {extramsg}
 SL: --
 SL: user: {author}\n{ifgt(parents|count, 1,
 "SL: merging:\n{parents % 'SL:   {node|short}: {desc|firstline}\n'}")
 }{if(currentbookmark,
 "SL: bookmark '{currentbookmark}'\n")}{
 filechanges}
defaulttitle=<Replace this line with a title. Use 1 line only, 67 chars or less>
emptymsg={if(title, title, defaulttitle)}\n
 Summary: {summary}\n
 Test Plan: {testplan}\n
filechanges={ifgt(parents|count, 1, filechangesmerge,
 ifgt(files|count, filechangethreshold, filechangesplain, filechangesdetailed))}
filechangesmerge=
filechangethreshold=100
filechangesplain={
 file_adds % "SL: added {file}\n"}{
 file_mods % "SL: changed {file}\n"}{
 file_dels % "SL: removed {file}\n"}{
 if(files, "", "SL: no files changed\n")}
filechangesdetailed={ifeq(verbosity,"verbose",diff()|hgprefix,stat("status")|hgprefix)}\n

[copytrace]
fastcopytrace=True
maxmovescandidatestocheck=0

[perftweaks]
disablecasecheck=True

[profiling:background]
enabled=1
format=text
freq=1
minelapsed=20
output=blackbox
sort=inlinetime
statformat=hotpath
type=stat

[smartlog]
names=master main
repos=remote/
indentnonpublic=True

[rebase]
experimental.inmemory.nomergedriver=False
experimental.inmemory=True

[packs]
maxpacksize=8GB
maxpackfilecount=1024
maxdatapendingbytes=4GB
maxdatabytes=50GB
maxhistorybytes=20GB

[amend]
autorestack=no-conflict
autorestackmsg=automatically restacking children!

[absorb]
amendflag=stack

[hiddenerror]
message=attempted to access hidden commit {0}
hint=use 'sl log -r {0} --hidden' to see more details about this commit

[histedit]
defaultrev=limit(only(.) & draft(), 50)
dropmissing=True
linelen=1000

[morestatus]
show=True

[fastpartialmatch]
generationnumber=5

[clone]
default-destination-dir=$HOME
nativecheckout=True
# TODO(T131560043): enable Rust for clone
use-rust=False
nativepull=True

[commands]
naked-default.in-repo=sl
naked-default.no-repo=help

[scale]
largeworkingcopy=True

[runlog]
enable=True
boring-commands=debugrunlog config version debugdynamicconfig

[deprecate]
rev-option=true
clone-include-option=true
clone-exclude-option=true

[indexedlog]
data.max-bytes-per-log=7G

[pull]
httpbookmarks=True
httphashprefix=True
master-fastpath=True

[exchange]
httpcommitlookup=True

[scmstore]
backingstore=True
enableshim=True
contentstorefallback=False
lfsptrwrites=True
auxindexedlog=True

[nativecheckout]
usescmstore=True

[pager]
pager=internal:streampager

[config]
use-rust=true

[hooks]
post-pull.prmarker=sl debugprmarker

[init]
prefer-git=True
"###);

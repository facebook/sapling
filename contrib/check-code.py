#!/usr/bin/env python
#
# check-code - a style and portability checker for Mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""style and portability checker for Mercurial

when a rule triggers wrong, do one of the following (prefer one from top):
 * do the work-around the rule suggests
 * doublecheck that it is a false match
 * improve the rule pattern
 * add an ignore pattern to the rule (3rd arg) which matches your good line
   (you can append a short comment and match this, like: #re-raises)
 * change the pattern to a warning and list the exception in test-check-code-hg
 * ONLY use no--check-code for skipping entire files from external sources
"""

from __future__ import absolute_import, print_function

import glob
import keyword
import optparse
import os
import re
import sys


if sys.version_info[0] < 3:
    opentext = open
else:

    def opentext(f):
        return open(f, encoding="ascii")


try:
    xrange
except NameError:
    xrange = range
try:
    import re2
except ImportError:
    re2 = None


def compilere(pat, multiline=False):
    if multiline:
        pat = "(?m)" + pat
    if re2:
        try:
            return re2.compile(pat)
        except re2.error:
            pass
    return re.compile(pat)


# check "rules depending on implementation of repquote()" in each
# patterns (especially pypats), before changing around repquote()
_repquotefixedmap = {
    " ": " ",
    "\n": "\n",
    ".": "p",
    ":": "q",
    "%": "%",
    "\\": "b",
    "*": "A",
    "+": "P",
    "-": "M",
}


def _repquoteencodechr(i):
    if i > 255:
        return "u"
    c = chr(i)
    if c in _repquotefixedmap:
        return _repquotefixedmap[c]
    if c.isalpha():
        return "x"
    if c.isdigit():
        return "n"
    return "o"


_repquotett = "".join(_repquoteencodechr(i) for i in xrange(256))


def repquote(m):
    t = m.group("text")
    t = t.translate(_repquotett)
    return m.group("quote") + t + m.group("quote")


def reppython(m):
    comment = m.group("comment")
    if comment:
        l = len(comment.rstrip())
        return "#" * l + comment[l:]
    return repquote(m)


def repcomment(m):
    return m.group(1) + "#" * len(m.group(2))


def repccomment(m):
    t = re.sub(r"((?<=\n) )|\S", "x", m.group(2))
    return m.group(1) + t + "*/"


def repcallspaces(m):
    t = re.sub(r"\n\s+", "\n", m.group(2))
    return m.group(1) + t


def repinclude(m):
    return m.group(1) + "<foo>"


def rephere(m):
    t = re.sub(r"\S", "x", m.group(2))
    return m.group(1) + t


testpats = [
    [
        (r"\b(push|pop)d\b", "don't use 'pushd' or 'popd', use 'cd'"),
        (r"grep.*-q", "don't use 'grep -q', redirect to /dev/null"),
        (r"(?<!hg )grep.* -a", "don't use 'grep -a', use in-line python"),
        (r"sed.*-i", "don't use 'sed -i', use a temporary file (it breaks on OSX)"),
        (r"\becho\b.*\\n", "don't use 'echo \\n', use printf"),
        (r"echo -n", "don't use 'echo -n', use printf"),
        (r"(^|\|\s*)\bwc\b[^|]*$\n(?!.*\(re\))", "filter wc output"),
        (r"head -c", "don't use 'head -c', use 'dd'"),
        (r"tail -n", "don't use the '-n' option to tail, just use '-<num>'"),
        (r"sha1sum", "don't use sha1sum, use $TESTDIR/md5sum.py"),
        (r"ls.*-\w*R", "don't use 'ls -R', use 'find'"),
        (r"printf.*[^\\]\\([1-9]|0\d)", r"don't use 'printf \NNN', use Python"),
        (r"printf.*[^\\]\\x", "don't use printf \\x, use Python"),
        (r"rm -rf \*", "don't use naked rm -rf, target a directory"),
        (r"\[[^\]]+==", "[ foo == bar ] is a bashism, use [ foo = bar ] instead"),
        (r"(^|\|\s*)grep (-\w\s+)*[^|]*[(|]\w", "use egrep for extended grep syntax"),
        (r"(^|\|\s*)e?grep .*\\S", "don't use \\S in regular expression"),
        (r"(?<!!)/bin/", "don't use explicit paths for tools"),
        (r"[^\n]\Z", "no trailing newline"),
        (r"^source\b", "don't use 'source', use '.'"),
        (r"touch -d", "don't use 'touch -d', use 'touch -t' instead"),
        (r"\bls +[^|\n-]+ +-", "options to 'ls' must come before filenames"),
        (r"[^>\n]>\s*\$HGRCPATH", "don't overwrite $HGRCPATH, append to it"),
        (r"^stop\(\)", "don't use 'stop' as a shell function name"),
        (r"(\[|\btest\b).*-e ", "don't use 'test -e', use 'test -f'"),
        (r"\[\[\s+[^\]]*\]\]", "don't use '[[ ]]', use '[ ]'"),
        (r"^alias\b.*=", "don't use alias, use a function"),
        (r"if\s*!", "don't use '!' to negate exit status"),
        (r"/dev/u?random", "don't use entropy, use /dev/zero"),
        (r"do\s*true;\s*done", "don't use true as loop body, use sleep 0"),
        (
            r"sed (-e )?\'(\d+|/[^/]*/)i(?!\\\n)",
            "put a backslash-escaped newline after sed 'i' command",
        ),
        (r"^diff *-\w*[uU].*$\n(^  \$ |^$)", "prefix diff -u/-U with cmp"),
        (r"^\s+(if)? diff *-\w*[uU]", "prefix diff -u/-U with cmp"),
        (r'[\s="`\']python\s(?!bindings)', "don't use 'python', use '$PYTHON'"),
        (r"seq ", "don't use 'seq', use $TESTDIR/seq.py"),
        (r"\butil\.Abort\b", "directly use error.Abort"),
        (r"\|&", "don't use |&, use 2>&1"),
        (r"\w =  +\w", "only one space after = allowed"),
        (r"\bsed\b.*[^\\]\\n", "don't use 'sed ... \\n', use a \\ and a newline"),
        (r"env.*-u", "don't use 'env -u VAR', use 'unset VAR'"),
        (r"cp.* -r ", "don't use 'cp -r', use 'cp -R'"),
        (r"find.*-printf", "don't use 'find -printf', it doesn't exist on BSD find(1)"),
    ],
    # warnings
    [
        (r"^function", "don't use 'function', use old style"),
        (r"^diff.*-\w*N", "don't use 'diff -N'"),
        (r"\$PWD|\${PWD}", "don't use $PWD, use `pwd`"),
        (r'^([^"\'\n]|("[^"\n]*")|(\'[^\'\n]*\'))*\^', "^ must be quoted"),
        (r"kill (`|\$\()", "don't use kill, use killdaemons.py"),
    ],
]

testfilters = [
    (r"( *)(#([^!][^\n]*\S)?)", repcomment),
    (r"<<(\S+)((.|\n)*?\n\1)", rephere),
]

uprefix = r"^  \$ "
utestpats = [
    [
        (r"^(\S.*||  [$>] \S.*)[ \t]\n", "trailing whitespace on non-output"),
        (uprefix + r"(true|exit 0)", "explicit zero exit unnecessary"),
        (uprefix + r".*(?<!\[)\$\?", "explicit exit code checks unnecessary"),
        (
            uprefix + r".*\|\| echo.*(fail|error)",
            "explicit exit code checks unnecessary",
        ),
        (uprefix + r"set -e", "don't use set -e"),
        (uprefix + r"(\s|fi\b|done\b)", "use > for continued lines"),
        (
            uprefix + r".*:\.\S*/",
            "x:.y in a path does not work on msys, rewrite "
            "as x://.y, or see `hg log -k msys` for alternatives",
            r"-\S+:\.|" "# no-msys",  # -Rxxx
        ),  # in test-pull.t which is skipped on windows
        (r"^  [^$>].*27\.0\.0\.1", "use $LOCALIP not an explicit loopback address"),
        (
            r"^  (?![>$] ).*\$LOCALIP.*[^)]$",
            "mark $LOCALIP output lines with (glob) to help tests in BSD jails",
        ),
        (r"^  (cat|find): .*: \$ENOENT\$", "use test -f to test for file existence"),
        (r"^  diff -[^ -]*p", "don't use (external) diff with -p for portability"),
        (r" readlink ", "use readlink.py instead of readlink"),
        (
            r"^  [-+][-+][-+] .* [-+]0000 \(glob\)",
            "glob timezone field in diff output for portability",
        ),
        (
            r"^  @@ -[0-9]+ [+][0-9]+,[0-9]+ @@",
            "use '@@ -N* +N,n @@ (glob)' style chunk header for portability",
        ),
        (
            r"^  @@ -[0-9]+,[0-9]+ [+][0-9]+ @@",
            "use '@@ -N,n +N* @@ (glob)' style chunk header for portability",
        ),
        (
            r"^  @@ -[0-9]+ [+][0-9]+ @@",
            "use '@@ -N* +N* @@ (glob)' style chunk header for portability",
        ),
        (
            uprefix + r"hg( +-[^ ]+( +[^ ]+)?)* +extdiff"
            r"( +(-[^ po-]+|--(?!program|option)[^ ]+|[^-][^ ]*))*$",
            "use $RUNTESTDIR/pdiff via extdiff (or -o/-p for false-positives)",
        ),
    ],
    # warnings
    [
        (
            r"^  (?!.*\$LOCALIP|.*\$HGPORT)[^*?/\n]* \(glob\)$",
            "glob match with no glob string (?, *, /, and $LOCALIP)",
        )
    ],
]

# transform plain test rules to unified test's
for i in [0, 1]:
    for tp in testpats[i]:
        p = tp[0]
        m = tp[1]
        if p.startswith(r"^"):
            p = r"^  [$>] (%s)" % p[1:]
        else:
            p = r"^  [$>] .*(%s)" % p
        utestpats[i].append((p, m) + tp[2:])

# don't transform the following rules:
# "  > \t" and "  \t" should be allowed in unified tests
testpats[0].append((r"^( *)\t", "don't use tabs to indent"))
utestpats[0].append((r"^( ?)\t", "don't use tabs to indent"))

utestfilters = [
    (r"<<(\S+)((.|\n)*?\n  > \1)", rephere),
    (r"( +)(#([^!][^\n]*\S)?)", repcomment),
]

pypats = [
    [
        (
            r"^\s*def\s*\w+\s*\(.*,\s*\(",
            "tuple parameter unpacking not available in Python 3+",
        ),
        (r"lambda\s*\(.*,.*\)", "tuple parameter unpacking not available in Python 3+"),
        (r"(?<!def)\s+(cmp)\(", "cmp is not available in Python 3+"),
        (r"(?<!\.)\breduce\s*\(.*", "reduce is not available in Python 3+"),
        (
            r"\bdict\(.*=",
            "dict constructor is different in Py2 and 3 and is slower than {}",
            "dict-from-generator",
        ),
        (r"\.has_key\b", "dict.has_key is not available in Python 3+"),
        (r"\s<>\s", "<> operator is not available in Python 3+, use !="),
        (r'[^_]_\([ \t\n]*(?:"[^"]+"[ \t\n+]*)+%', "don't use % inside _()"),
        (r"[^_]_\([ \t\n]*(?:'[^']+'[ \t\n+]*)+%", "don't use % inside _()"),
        (
            r"^\s+(self\.)?[A-Za-z][a-z0-9]+[A-Z]\w* = ",
            "don't use camelcase in identifiers",
            r"#.*camelcase-required",
        ),
        (
            r"class\s[^( \n]+:",
            "old-style class, use class foo(object)",
            r"#.*old-style",
        ),
        (
            r"class\s[^( \n]+\(\):",
            "class foo() creates old style object, use class foo(object)",
            r"#.*old-style",
        ),
        (
            r"\b(%s)\("
            % "|".join(k for k in keyword.kwlist if k not in ("print", "exec")),
            "Python keyword is not a function",
        ),
        (r"[\x80-\xff]", "non-ASCII character literal"),
        (r'("\')\.format\(', "str.format() has no bytes counterpart, use %"),
        (r"raise Exception", "don't raise generic exceptions"),
        (
            r"raise [^,(]+, (\([^\)]+\)|[^,\(\)]+)$",
            "don't use old-style two-argument raise, use Exception(message)",
        ),
        (r' is\s+(not\s+)?["\'0-9-]', "object comparison with literal"),
        (
            r" [=!]=\s+(True|False|None)",
            "comparison with singleton, use 'is' or 'is not' instead",
        ),
        (r"^\s*(while|if) [01]:", "use True/False for constant Boolean expression"),
        (r"^\s*if False(:| +and)", "Remove code instead of using `if False`"),
        (
            r"(?:(?<!def)\s+|\()hasattr\(",
            "hasattr(foo, bar) is broken on py2, use util.safehasattr(foo, bar) "
            "instead",
            r"#.*hasattr-py3-only",
        ),
        (r"opener\([^)]*\).read\(", "use opener.read() instead"),
        (r"opener\([^)]*\).write\(", "use opener.write() instead"),
        (r"[\s\(](open|file)\([^)]*\)\.read\(", "use util.readfile() instead"),
        (r"[\s\(](open|file)\([^)]*\)\.write\(", "use util.writefile() instead"),
        (
            r"^[\s\(]*(open(er)?|file)\([^)]*\)",
            "always assign an opened file to a variable, and close it afterwards",
        ),
        (
            r"[\s\(](open|file)\([^)]*\)\.",
            "always assign an opened file to a variable, and close it afterwards",
        ),
        (r"(?i)descend[e]nt", "the proper spelling is descendAnt"),
        (r"\.debug\(\_", "don't mark debug messages for translation"),
        (r"\.strip\(\)\.split\(\)", "no need to strip before splitting"),
        (r"^\s*except\s*:", "naked except clause", r"#.*re-raises"),
        (
            r"^\s*except\s([^\(,]+|\([^\)]+\))\s*,",
            'legacy exception syntax; use "as" instead of ","',
        ),
        (r"release\(.*wlock, .*lock\)", "wrong lock release order"),
        (r"\bdef\s+__bool__\b", "__bool__ should be __nonzero__ in Python 2"),
        (
            r'os\.path\.join\(.*, *(""|\'\')\)',
            "use pathutil.normasprefix(path) instead of os.path.join(path, '')",
        ),
        (r"\s0[0-7]+\b", 'legacy octal syntax; use "0o" prefix instead of "0"'),
        # XXX only catch mutable arguments on the first line of the definition
        (r"def.*[( ]\w+=\{\}", "don't use mutable default arguments"),
        (r"\butil\.Abort\b", "directly use error.Abort"),
        (r"^@(\w*\.)?cachefunc", "module-level @cachefunc is risky, please avoid"),
        (r"\.next\(\)", "don't use .next(), use next(...)"),
        (
            r"([a-z]*).revision\(\1\.node\(",
            "don't convert rev to node before passing to revision(nodeorrev)",
        ),
    ],
    [],
]
corepypats = [
    [
        (r"^import atexit", "don't use atexit, use ui.atexit"),
        (r"^import Queue", "don't use Queue, use util.queue + util.empty"),
        (r"^import cStringIO", "don't use cStringIO.StringIO, use util.stringio"),
        (r"^import urllib", "don't use urllib, use util.urlreq/util.urlerr"),
        (r"^import SocketServer", "don't use SockerServer, use util.socketserver"),
        (r"^import urlparse", "don't use urlparse, use util.urlreq"),
        (r"^import xmlrpclib", "don't use xmlrpclib, use util.xmlrpclib"),
        (r"^import cPickle", "don't use cPickle, use util.pickle"),
        (r"^import pickle", "don't use pickle, use util.pickle"),
        (r"^import httplib", "don't use httplib, use util.httplib"),
        (r"^import BaseHTTPServer", "use util.httpserver instead"),
        (
            r"^(from|import) mercurial\.(cext|pure|cffi)",
            "use mercurial.policy.importmod instead",
        ),
        (r"platform\.system\(\)", "don't use platform.system(), use pycompat"),
        # rules depending on implementation of repquote()
        (r' x+[xpqo%APM][\'"]\n\s+[\'"]x', "string join across lines with no space"),
        (
            r'''(?x)ui\.(status|progress|write|note|warn)\(
         [ \t\n#]*
         (?# any strings/comments might precede a string, which
           # contains translatable message)
         ((['"]|\'\'\'|""")[ \npq%bAPMxno]*(['"]|\'\'\'|""")[ \t\n#]+)*
         (?# sequence consisting of below might precede translatable message
           # - formatting string: "% 10s", "%05d", "% -3.2f", "%*s", "%%" ...
           # - escaped character: "\\", "\n", "\0" ...
           # - character other than '%', 'b' as '\', and 'x' as alphabet)
         (['"]|\'\'\'|""")
         ((%([ n]?[PM]?([np]+|A))?x)|%%|b[bnx]|[ \nnpqAPMo])*x
         (?# this regexp can't use [^...] style,
           # because _preparepats forcibly adds "\n" into [^...],
           # even though this regexp wants match it against "\n")''',
            "missing _() in ui message (use () to hide false-positives)",
        ),
    ],
    # warnings
    [
        # rules depending on implementation of repquote()
        (r"(^| )pp +xxxxqq[ \n][^\n]", "add two newlines after '.. note::'")
    ],
]

# XXX: Historic foo_bar naming. This is to make test-check-code clean, free
# from line numbers. Avoid adding new methods here if possible.
underscorenames = """
abort_report add_dirs add_files add_password annotate_highlight audit_git_path
audit_hg_path auth_getkey auth_getuserpasswd build_opener call_conduit
check_heads check_min_time check_perm clone_sparse close_all close_connection
collect_children commit_octopus compare_range conduit_config connect_ftp
convert_git_int_mode convert_rev create_server decode_guess del_all_files
delete_path determine_wants dirs_of do_hgweb do_open do_relink do_update
do_write exists_client export_commits export_git_objects export_hg_commit
export_hg_tags extract_from_parent extract_hg_metadata fetch_pack
filerevision_highlight filter_cset_known_bug_ids filter_min_date
filter_real_bug_ids filter_refs find_bugs find_incoming find_spec
find_stored_password find_sync_regions find_unconflicted find_user_password
finish_report fix_newline fns_generator ftp_open generate_css
generate_repo_subclass generate_ssh_vendor get_all get_bug_comments
get_bugzilla_user get_changed_refs get_contact get_data get_exportable get_file
get_filelogs_at_cl get_filelogs_to_sync get_files_changed get_git_author
get_git_incoming get_git_message_and_extra get_git_parents get_heads
get_latest_cl get_localname get_log_child get_longdesc_id get_matching_blocks
get_method get_mtime get_path_tag get_ready_conn get_refs get_stat get_times
get_transport_and_path get_ui get_unseen_commits get_user_id
get_valid_git_username_email git_cleanup git_file_readlines handle_read
has_automv has_section http_error_auth_reqed http_open http_request https_open
https_request import_commits import_git_commit import_git_objects import_tags
init_author_file is_octopus_part is_reachable layout_from_name link_path
load_cert_chain load_default_certs load_map load_remote_refs load_state
load_tags load_verify_locations log_error log_message log_request map_committer
map_git_get map_hg_get map_set mark_not_wanted mark_wanted merge_groups
merge_lines merge_regions monkeypatch_method offset_type open_connections
open_local_file pack_dirstate parse_changes parse_cl parse_client
parse_dirstate parse_filelist parse_filelist_at_cl parse_filelog parse_filelogs
parse_fstat parse_gitmodules parse_hgsub parse_hgsubstate parse_info
parse_subrepos parse_usermap parse_where print_time process_request proxy_open
pull_shallow put_inmemory range_header_to_tuple range_tuple_normalize
range_tuple_to_header read_allowed read_context_hunk read_pkt_refs
read_unified_hunk remote_name remote_refs replacelines_vec reset_retry_count
retry_http_basic_auth revset_fromgit revset_gitnode root_tree_sha run_command
run_wsgi save_map save_state save_tags screen_size send_bug_modify_email
send_cookies send_headers serialize_hgsub serialize_hgsubstate serve_cleanup
serve_forever serve_one set_ciphers set_commiter_from_author set_data set_path
set_ready set_ui setup_streamout show_changeset source_to_code
split_remote_name sql_buglist start_response stream_in_shallow stream_out
stream_out_shallow stream_wrap swap_out_encoding test_timeout transform_notgit
tree_entry update_changeset update_hg_bookmarks update_references
update_remote_branches upload_pack upstream_revs wrap_socket write_err
write_rej zc_create_server

active_diff add_padding_line address_family alias_default all_exportable
all_files allhunks_re all_objects allowed_opts allow_read allow_reuse_address
amend_copies amended_ctx app_id app_token arch_version audit_path authors_path
background_cmd bad_chars bad_headers base_marker base_ofs best_len best_node
best_rev bg_height blob_id branch_batches bullet_points ca_certs changed_files
changed_refs change_totals check_binary chunk_left cl_count cl_path
cmdline_message collapse_recursion commit_cache commit_count commit_hash
commit_info commit_re common_file common_nodes conduit_host conduit_path
conduit_protocol content_type copied_path copies_map copyfrom_path cur_len
cur_line current_ctx current_file custom_sections default_books default_path
default_port default_scm_daemon_port delim_re deny_read desc_lines
detect_renames diffgit_re diff_re diffs_seen dirty_trees dry_run
enableprofile_pat end_cut end_marker equal_a equal_b error_msg e_size exc_info
excluded_files exclude_pat existing_local_bms extra_defaults extra_in_message
extra_interline extra_message file_added_re files_map files_to_commit
fillchar_width find_copies_harder first_empty fix_nodeline_tail fmtmax_date
fmtmin_date formatted_args fqdn_re fresh_instance get_args get_binary
git_commit_tree git_extra git_extraitems git_fn git_match gitmodules_content
git_sha guard_re has_hunks headers_sent headers_set heads_hash hexnode_first
hexnode_last hg_commit_message hg_field hidden_count histedit_nodes include_pat
incoming_str indentation_level indent_string infinitepush_bgssh iterable_map
last_cut last_key lc_all len_a len_b len_base linear_search_result lines_re
link_prefix listdir_batch_size load_matcher local_hostname local_opts lookup_ch
map_file map_git_real map_hg map_hg_real matcher_opts match_opts max_cost
max_date max_files max_noise max_offset max_width metadata_key_value min_date
mode_state module_name msng_list name_idx nametype_idx newfile_re new_hash
new_header new_map new_ref new_refs new_socket new_tunnel next_revs
node_expander no_list numeric_loglevel num_lines num_pages obsstore_size
offset_end ok_sources ok_types old_guarded old_header old_ref old_refs old_rev
old_sha old_unapplied orig_amend_copies orig_data orig_encoded orig_encoding
original_series orig_paths orig_start orig_type other_bms out_file page_height
para_re parent_tree parsed_rev parsed_url permitted_opts pg_style pipei_bufsize
prefix_char prefix_end pretty_re printed_file profile_directory put_args
put_binary range_tup raw_url real_part real_parts ref_name remote_bm_names
remote_idx remote_info rename_detector renamed_out repo_callsign repo_index
repo_parts repos_to_update request_type response_class resp_url return_code
rev_first rev_last rev_numbers root_paths saved_status scratch_bms seen_dirs
sender_addr server_capabilities shift_interline shift_size silent_worker
sm_path sm_url space_left space_re special_re start_marker start_time
stdout_lines st_mode string_list stripped_refs str_lgt svn_config
switch_slashes tag_refname tags_file their_heads todo_total to_export to_pass
to_store total_bytes tree_sha tunnel_host tv_sec_ofs upstream_names
upstream_tips uptodate_annotated_tags url_scheme uuid_re version_info
without_newline

action_type added_files clean_files conflict_paths
create_clone_of_internal_map create_eden_dirstate deleted_files display_mode
eden_files explicit_matches get_merge_string get_merge_string ignored_files
manifest_entry max_to_show merge_state merge_str modified_files
non_removed_matches nonnormal_copy num_remaining orig_pack orig_unpack
parent_mf readlink_retry_estale removed_files to_remove total_conflicts
unknown_files unsure_files why_not_eden wrap_pack wrap_unpack

always_allow_pending always_allow_shared_pending chunked_paths cmd_cat_file
cmd_fetch_tree cmd_function cmd_manifest cmd_manifest_node_for_commit
cmd_old_cat_file data_fmt data_length dump_manifest eden_import_helper
fetch_tree file_rev files_data get_manifest_node header_data header_fields
is_last length_data lengths_fmt local_ui manifest_node node_hash num_paths
options_chunk os_mode path_lengths prefetch_files repo_name repo_ui rev_hash
rev_name rev_range send_chunk send_error send_exception treemanifest_paths
txn_id use_mononoke use_treemanifest

""".split()

# ported from check-commit
pycorepats = [
    [
        (
            r"\bdef (?!cffi|%s)[a-z]+_[a-z][a-z_]*\(" % "|".join(underscorenames),
            "use foobar, not foo_bar naming",
        ),
        (
            r"    (?!cffi|%s)[a-z]+_[a-z][a-z_]* = " % "|".join(underscorenames),
            "use foobar, not foo_bar naming",
        ),
    ],
    # warnings
    [],
]

pyfilters = [
    (
        r"""(?msx)(?P<comment>\#.*?$)|
         ((?P<quote>('''|\"\"\"|(?<!')'(?!')|(?<!")"(?!")))
          (?P<text>(([^\\]|\\.)*?))
          (?P=quote))""",
        reppython,
    )
]

# non-filter patterns
pynfpats = [
    [
        (r'pycompat\.osname\s*[=!]=\s*[\'"]nt[\'"]', "use pycompat.iswindows"),
        (r'pycompat\.osname\s*[=!]=\s*[\'"]posix[\'"]', "use pycompat.isposix"),
        (r'pycompat\.sysplatform\s*[!=]=\s*[\'"]darwin[\'"]', "use pycompat.isdarwin"),
    ],
    # warnings
    [],
]

# extension non-filter patterns
pyextnfpats = [
    [(r'^"""\n?[A-Z]', "don't capitalize docstring title")],
    # warnings
    [],
]

txtfilters = []

txtpats = [
    [
        ("\s$", "trailing whitespace"),
        (".. note::[ \n][^\n]", "add two newlines after note::"),
    ],
    [],
]

cpats = [
    [
        # rules depending on implementation of repquote()
    ],
    # warnings
    [
        # rules depending on implementation of repquote()
    ],
]

cfilters = [
    (r"(/\*)(((\*(?!/))|[^*])*)\*/", repccomment),
    (r"""(?P<quote>(?<!")")(?P<text>([^"]|\\")+)"(?!")""", repquote),
    (r"""(#\s*include\s+<)([^>]+)>""", repinclude),
    (r"(\()([^)]+\))", repcallspaces),
]

inutilpats = [
    [(r"\bui\.", "don't use ui in util")],
    # warnings
    [],
]

inrevlogpats = [
    [(r"\brepo\.", "don't use repo in revlog")],
    # warnings
    [],
]

webtemplatefilters = []

webtemplatepats = [
    [],
    [
        (
            r"{desc(\|(?!websub|firstline)[^\|]*)+}",
            "follow desc keyword with either firstline or websub",
        )
    ],
]

allfilesfilters = []

allfilespats = [
    [
        (
            r"(http|https)://[a-zA-Z0-9./]*selenic.com/",
            "use mercurial-scm.org domain URL",
        ),
        (
            r"mercurial@selenic\.com",
            "use mercurial-scm.org domain for mercurial ML address",
        ),
        (
            r"mercurial-devel@selenic\.com",
            "use mercurial-scm.org domain for mercurial-devel ML address",
        ),
    ],
    # warnings
    [],
]

py3pats = [
    [
        (r"os\.environ", "use encoding.environ instead (py3)", r"#.*re-exports"),
        (r"os\.name", "use pycompat.osname instead (py3)"),
        (r"os\.getcwd", "use pycompat.getcwd instead (py3)"),
        (r"os\.sep", "use pycompat.ossep instead (py3)"),
        (r"os\.pathsep", "use pycompat.ospathsep instead (py3)"),
        (r"os\.altsep", "use pycompat.osaltsep instead (py3)"),
        (r"sys\.platform", "use pycompat.sysplatform instead (py3)"),
        (r"getopt\.getopt", "use pycompat.getoptb instead (py3)"),
        (r"os\.getenv", "use encoding.environ.get instead"),
        (r"os\.setenv", "modifying the environ dict is not preferred"),
    ],
    # warnings
    [],
]

checks = [
    ("python", r".*\.(py|cgi)$", r"^#!.*python", pyfilters, pypats),
    ("python", r"edenscm.*\.(py|cgi)$", r"^#!.*python", pyfilters, corepypats),
    ("python", r".*\.(py|cgi)$", r"^#!.*python", [], pynfpats),
    ("python", r".*hgext.*\.py$", "", [], pyextnfpats),
    (
        "python",
        r".*(hgext|mercurial)/(?!demandimport|policy|pycompat).*\.py",
        "",
        pyfilters,
        pycorepats,
    ),
    (
        "python 3",
        r".*(hgext|mercurial)/(?!demandimport|policy|pycompat).*\.py",
        "",
        pyfilters,
        py3pats,
    ),
    ("test script", r"(.*/)?test-[^.~]*$", "", testfilters, testpats),
    ("c", r".*\.[ch]$", "", cfilters, cpats),
    ("unified test", r".*\.t$", "", utestfilters, utestpats),
    (
        "layering violation repo in revlog",
        r"mercurial/revlog\.py",
        "",
        pyfilters,
        inrevlogpats,
    ),
    ("layering violation ui in util", r"mercurial/util\.py", "", pyfilters, inutilpats),
    ("txt", r".*\.txt$", "", txtfilters, txtpats),
    (
        "web template",
        r"mercurial/templates/.*\.tmpl",
        "",
        webtemplatefilters,
        webtemplatepats,
    ),
    ("all except for .po", r".*(?<!\.po)$", "", allfilesfilters, allfilespats),
]


def _preparepats():
    for c in checks:
        failandwarn = c[-1]
        for pats in failandwarn:
            for i, pseq in enumerate(pats):
                # fix-up regexes for multi-line searches
                p = pseq[0]
                # \s doesn't match \n
                p = re.sub(r"(?<!\\)\\s", r"[ \\t]", p)
                # [^...] doesn't match newline
                p = re.sub(r"(?<!\\)\[\^", r"[^\\n", p)

                pats[i] = (re.compile(p, re.MULTILINE),) + pseq[1:]
        filters = c[3]
        for i, flt in enumerate(filters):
            filters[i] = re.compile(flt[0]), flt[1]


class norepeatlogger(object):
    def __init__(self):
        self._lastseen = None

    def log(self, fname, lineno, line, msg, blame):
        """print error related a to given line of a given file.

        The faulty line will also be printed but only once in the case
        of multiple errors.

        :fname: filename
        :lineno: line number
        :line: actual content of the line
        :msg: error message
        """
        msgid = fname, lineno, line
        if msgid != self._lastseen:
            if blame:
                print(
                    "%s:%d (%s): %s --> %s" % (fname, lineno, blame, msg, line.strip())
                )
            else:
                print("%s:%d: %s --> %s" % (fname, lineno, msg, line.strip()))
            self._lastseen = msgid


_defaultlogger = norepeatlogger()


def getblame(f):
    lines = []
    for l in os.popen("hg annotate -un %s" % f):
        start, line = l.split(":", 1)
        user, rev = start.split()
        lines.append((line[1:-1], user, rev))
    return lines


def checkfile(
    f,
    logfunc=_defaultlogger.log,
    maxerr=None,
    warnings=False,
    blame=False,
    debug=False,
    lineno=True,
):
    """checks style and portability of a given file

    :f: filepath
    :logfunc: function used to report error
              logfunc(filename, linenumber, linecontent, errormessage)
    :maxerr: number of error to display before aborting.
             Set to false (default) to report all errors

    return True if no error is found, False otherwise.
    """
    blamecache = None
    result = True

    try:
        with opentext(f) as fp:
            try:
                pre = post = fp.read()
            except UnicodeDecodeError as e:
                print("%s while reading %s" % (e, f))
                return result
    except IOError as e:
        print("Skipping %s, %s" % (f, str(e).split(":", 1)[0]))
        return result

    for name, match, magic, filters, pats in checks:
        post = pre  # discard filtering result of previous check
        if debug:
            print(name, f)
        fc = 0
        if not (re.match(match, f) or (magic and re.search(magic, pre))):
            if debug:
                print("Skipping %s for %s it doesn't match %s" % (name, match, f))
            continue
        if "no-" "check-code" in pre:
            # If you're looking at this line, it's because a file has:
            # no- check- code
            # but the reason to output skipping is to make life for
            # tests easier. So, instead of writing it with a normal
            # spelling, we write it with the expected spelling from
            # tests/test-check-code.t
            print("Skipping %s it has no-che?k-code (glob)" % f)
            return "Skip"  # skip checking this file
        for p, r in filters:
            post = re.sub(p, r, post)
        nerrs = len(pats[0])  # nerr elements are errors
        if warnings:
            pats = pats[0] + pats[1]
        else:
            pats = pats[0]
        # print post # uncomment to show filtered version

        if debug:
            print("Checking %s for %s" % (name, f))

        prelines = None
        errors = []
        for i, pat in enumerate(pats):
            if len(pat) == 3:
                p, msg, ignore = pat
            else:
                p, msg = pat
                ignore = None
            if i >= nerrs:
                msg = "warning: " + msg

            pos = 0
            n = 0
            for m in p.finditer(post):
                if prelines is None:
                    prelines = pre.splitlines()
                    postlines = post.splitlines(True)

                start = m.start()
                while n < len(postlines):
                    step = len(postlines[n])
                    if pos + step > start:
                        break
                    pos += step
                    n += 1
                l = prelines[n]

                if ignore and re.search(ignore, l, re.MULTILINE):
                    if debug:
                        print("Skipping %s for %s:%s (ignore pattern)" % (name, f, n))
                    continue
                bd = ""
                if blame:
                    bd = "working directory"
                    if not blamecache:
                        blamecache = getblame(f)
                    if n < len(blamecache):
                        bl, bu, br = blamecache[n]
                        if bl == l:
                            bd = "%s@%s" % (bu, br)

                errors.append((f, lineno and n + 1, l, msg, bd))
                result = False

        errors.sort()
        for e in errors:
            logfunc(*e)
            fc += 1
            if maxerr and fc >= maxerr:
                print(" (too many errors, giving up)")
                break

    return result


def main():
    parser = optparse.OptionParser("%prog [options] [files | -]")
    parser.add_option(
        "-w", "--warnings", action="store_true", help="include warning-level checks"
    )
    parser.add_option("-p", "--per-file", type="int", help="max warnings per file")
    parser.add_option(
        "-b", "--blame", action="store_true", help="use annotate to generate blame info"
    )
    parser.add_option("", "--debug", action="store_true", help="show debug information")
    parser.add_option(
        "",
        "--nolineno",
        action="store_false",
        dest="lineno",
        help="don't show line numbers",
    )

    parser.set_defaults(
        per_file=15, warnings=False, blame=False, debug=False, lineno=True
    )
    (options, args) = parser.parse_args()

    if len(args) == 0:
        check = glob.glob("*")
    elif args == ["-"]:
        # read file list from stdin
        check = sys.stdin.read().splitlines()
    else:
        check = args

    _preparepats()

    ret = 0
    for f in check:
        if not checkfile(
            f,
            maxerr=options.per_file,
            warnings=options.warnings,
            blame=options.blame,
            debug=options.debug,
            lineno=options.lineno,
        ):
            ret = 1
    return ret


if __name__ == "__main__":
    sys.exit(main())

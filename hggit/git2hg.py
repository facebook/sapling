# git2hg.py - convert Git repositories and commits to Mercurial ones

import urllib
from dulwich.objects import Commit, Tag

def find_incoming(git_object_store, git_map, refs):
    '''find what commits need to be imported

    git_object_store is a dulwich object store.
    git_map is a map with keys being Git commits that have already been imported
    refs is a map of refs to SHAs that we're interested in.'''

    done = set()
    commit_cache = {}

    # sort by commit date
    def commitdate(sha):
        obj = git_object_store[sha]
        return obj.commit_time-obj.commit_timezone

    # get a list of all the head shas
    def get_heads(refs):
        todo = []
        seenheads = set()
        for sha in refs.itervalues():
            # refs could contain refs on the server that we haven't pulled down
            # the objects for
            if sha in git_object_store:
                obj = git_object_store[sha]
                while isinstance(obj, Tag):
                    obj_type, sha = obj.object
                    obj = git_object_store[sha]
                if isinstance(obj, Commit) and sha not in seenheads:
                    seenheads.add(sha)
                    todo.append(sha)

        todo.sort(key=commitdate, reverse=True)
        return todo

    def get_unseen_commits(todo):
        '''get all unseen commits reachable from todo in topological order

        'unseen' means not reachable from the done set and not in the git map.
        Mutates todo and the done set in the process.'''
        commits = []
        while todo:
            sha = todo[-1]
            if sha in done or sha in git_map:
                todo.pop()
                continue
            assert isinstance(sha, str)
            if sha in commit_cache:
                obj = commit_cache[sha]
            else:
                obj = git_object_store[sha]
                commit_cache[sha] = obj
            assert isinstance(obj, Commit)
            for p in obj.parents:
                if p not in done and p not in git_map:
                    todo.append(p)
                    # process parents of a commit before processing the
                    # commit itself, and come back to this commit later
                    break
            else:
                commits.append(sha)
                done.add(sha)
                todo.pop()

        return commits

    todo = get_heads(refs)
    commits = get_unseen_commits(todo)

    return GitIncomingResult(commits, commit_cache)

class GitIncomingResult(object):
    '''struct to store result from find_incoming'''
    def __init__(self, commits, commit_cache):
        self.commits = commits
        self.commit_cache = commit_cache

def extract_hg_metadata(message, git_extra):
    split = message.split("\n--HG--\n", 1)
    # Renames are explicitly stored in Mercurial but inferred in Git. For
    # commits that originated in Git we'd like to optionally infer rename
    # information to store in Mercurial, but for commits that originated in
    # Mercurial we'd like to disable this. How do we tell whether the commit
    # originated in Mercurial or in Git? We rely on the presence of extra hg-git
    # fields in the Git commit.
    # - Commits exported by hg-git versions past 0.7.0 always store at least one
    #   hg-git field.
    # - For commits exported by hg-git versions before 0.7.0, this becomes a
    #   heuristic: if the commit has any extra hg fields, it definitely originated
    #   in Mercurial. If the commit doesn't, we aren't really sure.
    # If we think the commit originated in Mercurial, we set renames to a
    # dict. If we don't, we set renames to None. Callers can then determine
    # whether to infer rename information.
    renames = None
    extra = {}
    branch = None
    if len(split) == 2:
        renames = {}
        message, meta = split
        lines = meta.split("\n")
        for line in lines:
            if line == '':
                continue

            if ' : ' not in line:
                break
            command, data = line.split(" : ", 1)

            if command == 'rename':
                before, after = data.split(" => ", 1)
                renames[after] = before
            if command == 'branch':
                branch = data
            if command == 'extra':
                k, v = data.split(" : ", 1)
                extra[k] = urllib.unquote(v)

    git_fn = 0
    for field, data in git_extra:
        if field.startswith('HG:'):
            if renames is None:
                renames = {}
            command = field[3:]
            if command == 'rename':
                before, after = data.split(':', 1)
                renames[urllib.unquote(after)] = urllib.unquote(before)
            elif command == 'extra':
                k, v = data.split(':', 1)
                extra[urllib.unquote(k)] = urllib.unquote(v)
        else:
            # preserve ordering in Git by using an incrementing integer for
            # each field. Note that extra metadata in Git is an ordered list
            # of pairs.
            hg_field = 'GIT%d-%s' % (git_fn, field)
            git_fn += 1
            extra[urllib.quote(hg_field)] = urllib.quote(data)

    return (message, renames, branch, extra)

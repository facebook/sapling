# git2hg.py - convert Git repositories and commits to Mercurial ones

from dulwich.objects import Commit, Tag

def find_incoming(git_object_store, git_map, refs):
    '''find what commits need to be imported

    git_object_store is a dulwich object store.
    git_map is a map with keys being Git commits that have already been imported
    refs is a map of refs to SHAs that we're interested in.'''

    todo = []
    done = set()
    commit_cache = {}

    # get a list of all the head shas
    seenheads = set()
    for sha in refs.itervalues():
        # refs could contain refs on the server that we haven't pulled down the
        # objects for
        if sha in git_object_store:
            obj = git_object_store[sha]
            while isinstance(obj, Tag):
                obj_type, sha = obj.object
                obj = git_object_store[sha]
            if isinstance (obj, Commit) and sha not in seenheads:
                seenheads.add(sha)
                todo.append(sha)

    # sort by commit date
    def commitdate(sha):
        obj = git_object_store[sha]
        return obj.commit_time-obj.commit_timezone

    todo.sort(key=commitdate, reverse=True)

    # traverse the heads getting a list of all the unique commits in
    # topological order
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

    return GitIncomingResult(commits, commit_cache)

class GitIncomingResult(object):
    '''struct to store result from find_incoming'''
    def __init__(self, commits, commit_cache):
        self.commits = commits
        self.commit_cache = commit_cache

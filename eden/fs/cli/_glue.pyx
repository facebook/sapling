from libcpp.string cimport string

cdef extern from 'eden/fs/importer/git/GitImporter.h' namespace 'facebook::eden' nogil:
    string doGitImport(const string& repoPath, const string& dbPath)

def do_git_import(string repoPath, string dbPath):
    return doGitImport(repoPath, dbPath)

#!/usr/bin/env python2

# LICENSE
#
# Copyright (c) 2004, Francois Beausoleil
# All rights reserved.
#
# Redistribution and use in source and binary forms, with or without
# modification, are permitted provided that the following conditions
# are met:
#
#   * Redistributions of source code must retain the above copyright
#      notice, this list of conditions and the following disclaimer.
#   * Redistributions in binary form must reproduce the above copyright
#     notice, this list of conditions and the following disclaimer in
#     the documentation and/or other materials provided with the
#     distribution.
#   * Neither the name of the Francois Beausoleil nor the names of its
#     contributors may be used to endorse or promote products derived
#     from this software without specific prior written permission.
#
# THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
# "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
# LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
# A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
# OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
# SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
# LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
# DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
# THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
# (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
# OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import getopt
import sys
import os
import re
import traceback
from svn import core, repos, fs

VERSION='$Id: rsvn.py 20 2005-09-23 15:08:08Z fbos $'
RCOPY_RE    = re.compile('^\s*rcopy\s+(.+)\s+(.+)$')
RMOVE_RE    = re.compile('^\s*rmove\s+(.+)\s+(.+)$')
RMKDIR_RE   = re.compile('^\s*rmkdir\s+(.+)$')
RDELETE_RE  = re.compile('^\s*rdelete\s+(.+)$')
COMMENT_RE  = re.compile('(?:^\s*#)|(?:^\s*$)')

def usage(error=None):
  if error:
    print 'Error: %s\n' % error
  print 'USAGE: %s --message=MESSAGE repos_path [--username=USERNAME]' % (
          os.path.basename(sys.argv[0]))
  print ''
  print '  --help, -h           print this usage message and exit with success'
  print '  --version            print the version number'
  print '  --username=USERNAME  Username to execute the commands under'
  print '  --message=LOG_MSG    Log message to execute the commit with'
  print ''
  print 'Reads STDIN and parses the following commands, to execute them on the server, '
  print 'all within the same transaction:'
  print ''
  print '  rcopy SRC DEST       Copy the HEAD revision of a file or folder'
  print '  rmove SRC DEST       Copy + delete the HEAD revision of a file or folder'
  print '  rdelete TARGET       Deletes something from the repository'
  print '  rmkdir TARGET        Creates a new folder (must create parents first)'
  print '  #                    Initiates a comment'
  print ''
#        12345678901234567890123456789012345678901234567890123456789012345678901234567890


class Transaction:
  """Represents a transaction in a Subversion repository
  
     Transactions are long-lived objects which exist in the repository,
     and are used to build an intermediate representation of a new
     revision.  Once the transaction is committed, the repository
     bumps the revision number, and links the new transaction in the
     Subversion filesystem."""

  def __init__(self, repository, rev, username, message, pool, logger=None):
    if logger:
     self.logger = logger
    else:
     self.logger = sys.stdout
    self.pool = pool
    self.rev = rev

    self.fsptr = repos.svn_repos_fs(repository)
    self.rev_root = fs.revision_root(self.fsptr, self.rev,
                    self.pool)
    self.txnp = repos.svn_repos_fs_begin_txn_for_commit(
            repository, self.rev, username, message, self.pool)
    self.txn_root = fs.txn_root(self.txnp, self.pool)
    self.log('Base revision %d\n' % rev)

  def commit(self):
    values = fs.commit_txn(self.txnp, self.pool)
    return values[1]

  def rollback(self):
    fs.abort_txn(self.txnp, self.pool)

  def copy(self, src, dest, subpool):
    self.log('A  + %s\n' % dest)
    fs.copy(self.rev_root, src, self.txn_root, dest, subpool)

  def delete(self, entry, subpool):
    self.log('D    %s\n' % entry)
    fs.delete(self.txn_root, entry, subpool)

  def mkdir(self, entry, subpool):
    self.log('A    %s\n' % entry)
    fs.make_dir(self.txn_root, entry, subpool)

  def move(self, src, dest, subpool):
    self.copy(src, dest, subpool)
    self.delete(src, subpool)

  def log(self, msg):
    self.logger.write(msg)


class Repository:
  """Represents a Subversion repository, and allows common operations
     on it."""

  def __init__(self, repos_path, pool, logger=None):
    if logger:
      self.logger = logger
    else:
      self.logger = sys.stdout
    self.pool = pool
    assert self.pool

    self.repo = repos.svn_repos_open(repos_path, self.pool)
    self.fsptr = repos.svn_repos_fs(self.repo)

  def get_youngest(self):
    """Returns the youngest revision in the repository."""
    return fs.youngest_rev(self.fsptr, self.pool)

  def begin(self, username, log_msg):
    """Initiate a new Transaction"""
    return Transaction(self.repo, self.get_youngest(), username,
            log_msg, self.pool, self.logger)

  def close(self):
    """Close the repository, aborting any uncommitted transactions"""
    core.svn_pool_destroy(self.pool)
    core.apr_terminate()

  def subpool(self):
    """Instantiates a new pool from the master pool"""
    return core.svn_pool_create(self.pool)

  def delete_pool(self, pool):
    """Deletes the passed-in pool.  Returns None, to assign to pool in
       caller."""
    core.svn_pool_destroy(pool)
    return None

def rsvn(pool):
  log_msg = None

  try:
    opts, args = getopt.getopt(sys.argv[1:], 'vh',
                ["help", "username=", "message=", "version"])
  except getopt.GetoptError, e:
    sys.stderr.write(str(e) + '\n\n')
    usage()
    sys.exit(1)
  
  for opt, value in opts:
    if opt == '--version':
      print '%s version %s' % (os.path.basename(sys.argv[0]), VERSION)
      sys.exit(0)
    elif opt == '--help' or opt == '-h':
      usage()
      sys.exit(0)
    elif opt == '--username':
      username = value
    elif opt == '--message':
      log_msg = value
  
  if log_msg == None:
    usage('Missing --message argument')
    sys.exit(1)
  
  if len(args) != 1:
    usage('Missing repository path argument')
    sys.exit(1)
  
  repos_path = args[0]
  print 'Accessing repository at [%s]' % repos_path

  repository = Repository(repos_path, pool)
  sub = repository.subpool()
  
  try:
    txn = repository.begin(username, log_msg)
  
    # Read commands from STDIN
    lineno = 0
    for line in sys.stdin:
      lineno += 1
      
      core.svn_pool_clear(sub)
      try:
        if COMMENT_RE.search(line):
          continue
    
        match = RCOPY_RE.search(line)
        if match:
          src = match.group(1)
          dest = match.group(2)
          txn.copy(src, dest, sub)
          continue
    
        match = RMOVE_RE.search(line)
        if match:
          src = match.group(1)
          dest = match.group(2)
          txn.move(src, dest, sub)
          continue
    
        match = RMKDIR_RE.search(line)
        if match:
          entry = match.group(1)
          txn.mkdir(entry, sub)
          continue
    
        match = RDELETE_RE.search(line)
        if match:
          entry = match.group(1)
          txn.delete(entry, sub)
          continue
  
        raise NameError, ('Unknown command [%s] on line %d' %
            (line, lineno))
    
      except:
        sys.stderr.write(('Exception occured while processing line %d:\n' % 
            lineno))
        etype, value, tb = sys.exc_info()
        traceback.print_exception(etype, value, tb, None, sys.stderr)
        sys.stderr.write('\n')
        txn.rollback()
        sys.exit(1)
  
    new_rev = txn.commit()
    print '\nCommitted revision %d.' % new_rev
  
  finally:
    print '\nRepository closed.'

def main():
    core.run_app(rsvn)

if __name__ == '__main__':
    main()

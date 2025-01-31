# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config

# mononoke_admin ground truth: exit code is only logged on failure
  $ mononoke_admin list-repos
  0 repo
  $ mononoke_admin invalid
  error: unrecognized subcommand 'invalid'
  
  Usage: admin [OPTIONS] <--config-path <CONFIG_PATH>|--config-tier <CONFIG_TIER>|--prod|--git-config> <COMMAND>
  
  For more information, try '--help'.
  [2]


# mononoke_testtool ground truth: exit code is only logged on failure
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_testtool invalid
  error: unrecognized subcommand 'invalid'
  
  Usage: testtool [OPTIONS] <--config-path <CONFIG_PATH>|--config-tier <CONFIG_TIER>|--prod|--git-config> <COMMAND>
  
  For more information, try '--help'.
  [2]


# strip_glog removes what we expect
  $ cat << EOF | strip_glog
  > some
  > I0131 01:16:33.803668 4035520 Logger.h:335] [WARNING] WhenceScribeLogged is overwritten to SANDBOX. https://fburl.com/cpp-logger-whence-logged
  > plain
  > E0130 16:56:31.642324 98667 Ods3Publisher.cpp:173] [ODS3 SDK] ODS3 SDK has dropped some samples. This is NOT the cause of errors in your application. 0 samples were dropped during observations. 16 samples were dropped due to publishing. One publishing error is: apache::thrift::transport::TTransportException: Channel is !good()
  > output
  > EOF
  some
  [WARNING] WhenceScribeLogged is overwritten to SANDBOX. https://fburl.com/cpp-logger-whence-logged
  plain
  output

# strip_glog will strip the exit code
  $ mononoke_admin invalid 2>&1 | strip_glog
  error: unrecognized subcommand 'invalid'
  
  Usage: admin [OPTIONS] <--config-path <CONFIG_PATH>|--config-tier <CONFIG_TIER>|--prod|--git-config> <COMMAND>
  
  For more information, try '--help'.


# with_stripped_logs removes what we expect
  $ with_stripped_logs cat << EOF
  > some
  > I0131 01:16:33.803668 4035520 Logger.h:335] [WARNING] WhenceScribeLogged is overwritten to SANDBOX. https://fburl.com/cpp-logger-whence-logged
  > plain
  > E0130 16:56:31.642324 98667 Ods3Publisher.cpp:173] [ODS3 SDK] ODS3 SDK has dropped some samples. This is NOT the cause of errors in your application. 0 samples were dropped during observations. 16 samples were dropped due to publishing. One publishing error is: apache::thrift::transport::TTransportException: Channel is !good()
  > output
  > EOF
  some
  [WARNING] WhenceScribeLogged is overwritten to SANDBOX. https://fburl.com/cpp-logger-whence-logged
  plain
  output

# with_stripped_logs doesn't propagate the exit code correctly
  $ with_stripped_logs mononoke_admin invalid
  error: unrecognized subcommand 'invalid'
  
  Usage: admin [OPTIONS] <--config-path <CONFIG_PATH>|--config-tier <CONFIG_TIER>|--prod|--git-config> <COMMAND>
  
  For more information, try '--help'.



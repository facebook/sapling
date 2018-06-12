/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/privhelper/PrivHelper.h"

#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include <sys/socket.h>
#include <chrono>
#include <vector>

#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/fuse/privhelper/test/PrivHelperTestServer.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly::string_piece_literals;
using namespace facebook::eden;
using namespace std::chrono_literals;
using facebook::eden::UserInfo;
using folly::ByteRange;
using folly::checkUnixError;
using folly::File;
using folly::IOBuf;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;
using std::string;
using std::vector;

void createTestConns(PrivHelperConn& sender, PrivHelperConn& receiver) {
  // Use the default createConnPair() function
  PrivHelperConn::createConnPair(sender, receiver);

  // Our tests are single threaded, and don't send and receive simultaneously.
  // Therefore the kernel socket buffers must be large enough to hold all data
  // we are trying to send, or our send call will block (since no one is
  // actively receiving on the other side).
  //
  // Set send timeouts on both sides so the test won't hang forever just
  // in case the socket buffers aren't large enough.
  struct timeval tv;
  tv.tv_sec = 3;
  tv.tv_usec = 0;
  int rc =
      setsockopt(sender.getSocket(), SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
  checkUnixError(rc, "failed to set privhelper socket send timeout");
  rc = setsockopt(
      receiver.getSocket(), SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
  checkUnixError(rc, "failed to set privhelper socket send timeout");
  // Set receive timeouts too, for good measure
  // createConnPair() will have already set a timeout on the client side
  // (our sender), but not the receiver.
  rc = setsockopt(sender.getSocket(), SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
  checkUnixError(rc, "failed to set privhelper socket receive timeout");
  rc = setsockopt(
      receiver.getSocket(), SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
  checkUnixError(rc, "failed to set privhelper socket receive timeout");
}

void checkReceivedMsg(
    const PrivHelperConn::Message& expected,
    const PrivHelperConn::Message& received) {
  // Make sure the received message header is identical
  EXPECT_EQ(expected.msgType, received.msgType);
  EXPECT_EQ(expected.xid, received.xid);
  EXPECT_EQ(expected.dataSize, received.dataSize);

  // Make sure the received body data is identical
  EXPECT_EQ(
      (ByteRange{expected.data, expected.dataSize}),
      (ByteRange{received.data, received.dataSize}));
}

void checkReceivedFD(int expected, int received) {
  EXPECT_NE(-1, received);

  // The received file descriptor shouldn't be numerically the same
  // as the expected fd, but it should refer to the exact same file.
  EXPECT_NE(expected, received);

  struct stat origStatInfo;
  int rc = fstat(expected, &origStatInfo);
  checkUnixError(rc, "failed to stat expected file descriptor");

  struct stat recvStatInfo;
  rc = fstat(received, &recvStatInfo);
  checkUnixError(rc, "failed to stat received file descriptor");
  EXPECT_EQ(origStatInfo.st_dev, recvStatInfo.st_dev);
  EXPECT_EQ(origStatInfo.st_ino, recvStatInfo.st_ino);
}

TEST(PrivHelper, SendFD) {
  PrivHelperConn sender;
  PrivHelperConn receiver;
  createTestConns(sender, receiver);

  PrivHelperConn::Message req;
  req.msgType = 19;
  req.xid = 92;
  // Just send some arbitrary bytes to make sure the low-level
  // sendMsg()/recvMsg() passes them through as-is.
  // We include a null byte and some other low bytes as well to
  // make sure it works with arbitrary binary data.
  uint8_t bodyBytes[] = "test1234\x00\x01\x02\x03\x04test";
  req.dataSize = sizeof(bodyBytes);
  memcpy(req.data, bodyBytes, req.dataSize);

  TemporaryFile tempFile;

  // Send the message
  sender.sendMsg(&req, tempFile.fd());

  // Receive it on the other socket
  PrivHelperConn::Message resp;
  File receivedFile;
  receiver.recvMsg(&resp, &receivedFile);

  // Check the received info
  checkReceivedMsg(req, resp);
  checkReceivedFD(tempFile.fd(), receivedFile.fd());
}

TEST(PrivHelper, PipelinedSend) {
  PrivHelperConn sender;
  PrivHelperConn receiver;
  createTestConns(sender, receiver);

  PrivHelperConn::Message req1;
  req1.msgType = 19;
  req1.xid = 92;
  req1.dataSize = 20;
  memset(req1.data, 'a', req1.dataSize);

  PrivHelperConn::Message req2;
  req2.msgType = 0;
  req2.xid = 123;
  req2.dataSize = sizeof(req2.data);
  memset(req2.data, 'b', req2.dataSize);

  TemporaryFile tempFile1;
  TemporaryFile tempFile2;

  // Make two separate sendMsg() calls before we try reading anything
  // from the receiver.
  sender.sendMsg(&req1, tempFile1.fd());
  sender.sendMsg(&req2, tempFile2.fd());

  // Now perform the receives, and make sure we receive each message separately
  PrivHelperConn::Message resp1;
  File rfile1;
  receiver.recvMsg(&resp1, &rfile1);
  {
    SCOPED_TRACE("request 1");
    checkReceivedMsg(req1, resp1);
    checkReceivedFD(tempFile1.fd(), rfile1.fd());
  }

  PrivHelperConn::Message resp2;
  File rfile2;
  receiver.recvMsg(&resp2, &rfile2);
  {
    SCOPED_TRACE("request 2");
    checkReceivedMsg(req2, resp2);
    checkReceivedFD(tempFile2.fd(), rfile2.fd());
  }
}

TEST(PrivHelper, RecvEOF) {
  PrivHelperConn sender;
  PrivHelperConn receiver;
  createTestConns(sender, receiver);

  sender.close();

  PrivHelperConn::Message msg;
  EXPECT_THROW(receiver.recvMsg(&msg, nullptr), PrivHelperClosedError);
}

void testSerializeMount(StringPiece mountPath) {
  PrivHelperConn::Message msg;
  msg.xid = 1;
  PrivHelperConn::serializeMountRequest(&msg, mountPath);

  string readMountPath;
  PrivHelperConn::parseMountRequest(&msg, readMountPath);
  EXPECT_EQ(mountPath.str(), readMountPath);
}

TEST(PrivHelper, SerializeMount) {
  testSerializeMount("/path/to/mount/point");
  testSerializeMount("foobar");
  testSerializeMount("");
  testSerializeMount(StringPiece("foo\0\0\0bar", 9));
}

void testSerializeTakeoverStartupRequest(
    StringPiece mountPath,
    const vector<string>& bindMounts) {
  PrivHelperConn::Message msg;
  msg.xid = 1;
  PrivHelperConn::serializeTakeoverStartupRequest(&msg, mountPath, bindMounts);

  string readMountPath;
  vector<string> readBindMounts;
  PrivHelperConn::parseTakeoverStartupRequest(
      &msg, readMountPath, readBindMounts);
  EXPECT_EQ(mountPath.str(), readMountPath);
  EXPECT_EQ(bindMounts, readBindMounts);
}

TEST(PrivHelper, SerializeTakeoverStartupRequest) {
  testSerializeTakeoverStartupRequest("/path/to/mount/point", vector<string>{});
  testSerializeTakeoverStartupRequest("foo", vector<string>{"a", "b"});
  testSerializeTakeoverStartupRequest(
      "foo\0\0\0bar"_sp, vector<string>{"a", "b"});
}

void testSerializeTakeoverShutdown(StringPiece mountPath) {
  PrivHelperConn::Message msg;
  msg.xid = 1;
  PrivHelperConn::serializeTakeoverShutdownRequest(&msg, mountPath);

  string readMountPath;
  PrivHelperConn::parseTakeoverShutdownRequest(&msg, readMountPath);
  EXPECT_EQ(mountPath.str(), readMountPath);
}

TEST(PrivHelper, SerializeTakeoverShutdownRequest) {
  testSerializeTakeoverShutdown("/path/to/mount/point");
  testSerializeTakeoverShutdown("foobar");
  testSerializeTakeoverShutdown("");
  testSerializeTakeoverShutdown("foo\0\0\0bar"_sp);
}

TEST(PrivHelper, SerializeError) {
  PrivHelperConn::Message msg;
  // Serialize an exception
  try {
    folly::throwSystemErrorExplicit(ENOENT, "test error");
  } catch (const std::exception& ex) {
    PrivHelperConn::serializeErrorResponse(&msg, ex);
  }

  // Try parsing it as a mount response
  try {
    PrivHelperConn::parseEmptyResponse(&msg);
    FAIL() << "expected parseEmptyResponse() to throw";
  } catch (const std::system_error& ex) {
    EXPECT_TRUE(isErrnoError(ex))
        << "unexpected error category: " << folly::exceptionStr(ex);
    EXPECT_EQ(ENOENT, ex.code().value());
    EXPECT_TRUE(strstr(ex.what(), "test error") != nullptr)
        << "unexpected error string: " << ex.what();
  }
}

TEST(PrivHelper, ServerShutdownTest) {
  TemporaryDirectory tmpDir;
  PrivHelperTestServer server;

  auto fooDir = tmpDir.path() / "foo";
  create_directory(fooDir);
  auto foo = fooDir.string();

  // Note we do not create this directory explicitly because we want to verify
  // that privilegedBindMount takes care of this for us.
  auto mountedBuckOut = tmpDir.path() / "foo" / "buck-out";

  auto barDir = tmpDir.path() / "bar";
  create_directory(barDir);
  auto bar = barDir.string();

  auto otherDir = (tmpDir.path() / "other");
  create_directory(barDir);
  auto other = otherDir.string();

  {
    auto privHelper = startPrivHelper(&server, UserInfo::lookup());

    // Create a few mount points
    privHelper->fuseMount(foo).get(50ms);
    privHelper->fuseMount(bar).get(50ms);
    EXPECT_TRUE(server.isMounted(foo));
    EXPECT_TRUE(server.isMounted(bar));
    EXPECT_FALSE(server.isMounted(other));

    // Create a bind mount.
    EXPECT_FALSE(boost::filesystem::exists(mountedBuckOut));
    TemporaryDirectory realBuckOut;
    privHelper->bindMount(realBuckOut.path().c_str(), mountedBuckOut.c_str())
        .get(50ms);
    EXPECT_TRUE(server.isBindMounted(mountedBuckOut.c_str()));
    EXPECT_TRUE(boost::filesystem::exists(mountedBuckOut))
        << "privilegedBindMount() should create the bind mount directory for "
           "the caller.";

    // The privhelper will exit at the end of this scope
  }

  // Make sure things get umounted when the privhelper quits
  EXPECT_FALSE(server.isMounted(foo));
  EXPECT_FALSE(server.isBindMounted(mountedBuckOut.string()));
  EXPECT_FALSE(server.isMounted(bar));
  EXPECT_FALSE(server.isMounted(other));
}

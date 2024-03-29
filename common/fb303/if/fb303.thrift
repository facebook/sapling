/**
 * fb303.thrift
 *
 * Copyright (c) 2006-present, Facebook, Inc.
 * Distributed under the Thrift Software License
 *
 * See accompanying file LICENSE or visit the Thrift site at:
 * http://developers.facebook.com/thrift/
 *
 *
 * Definition of common Facebook data types and status reporting mechanisms
 * common to all Facebook services. In some cases, these methods are
 * provided in the base implementation, and in other cases they simply define
 * methods that inheriting applications should implement (i.e. status report)
 *
 * @author Mark Slee <mcslee@facebook.com>
 */

namespace cpp facebook.fb303

/**
 * Common status reporting mechanism across all services
 */
include "thrift/annotation/thrift.thrift"

enum fb_status {
  DEAD = 0,
  STARTING = 1,
  ALIVE = 2,
  STOPPING = 3,
  STOPPED = 4,
  WARNING = 5,
}

/**
 * Standard base service
 */
service FacebookService {
  /**
   * Gets the status of this service
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  fb_status getStatus();

  /**
   * Gets the counters for this service
   */
  map<string, i64> getCounters();

  /**
   * Suggest a shutdown to the server
   */
  oneway void shutdown();

  /**
   * Returns the unix time that the server has been running since
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  i64 aliveSince();

  /**
   * Returns the pid of the process
   */
  i64 getPid();
}

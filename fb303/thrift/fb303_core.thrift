/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package "facebook.com/fb303"

namespace cpp2 "facebook.fb303.cpp2"
namespace py3 ""
namespace java com.facebook.fb303.core
namespace java.swift com.facebook.swift.fb303.core
namespace py.asyncio fb303_asyncio.fb303_core
namespace perl fb303
namespace hack fb303
namespace node_module fb303
namespace go fb303.thrift.fb303_core
namespace js fb303

include "thrift/annotation/cpp.thrift"
include "thrift/annotation/thrift.thrift"

/**
 * Common status reporting mechanism across all services
 */
enum fb303_status {
  DEAD = 0,
  STARTING = 1,
  ALIVE = 2,
  STOPPING = 3,
  STOPPED = 4,
  WARNING = 5,
}

/**
 * Standard base service interface.
 *
 * This interface provides methods to get some service metadata that is common
 * across many services.
 */
service BaseService {
  /**
   * Gets the status of this service
   */
  @thrift.DeprecatedUnvalidatedAnnotations{
    items = {"cpp.generate_deprecated_header_client_methods": "1"},
  }
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  fb303_status getStatus();

  /**
   * Returns a descriptive name of the service
   */
  @thrift.DeprecatedUnvalidatedAnnotations{
    items = {"cpp.generate_deprecated_header_client_methods": "1"},
  }
  string getName();

  /**
   * Returns the version of the service
   */
  string getVersion();

  /**
   * User friendly description of status, such as why the service is in
   * the dead or warning state, or what is being started or stopped.
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  string getStatusDetails();

  /**
   * Gets the counters for this service
   */
  @cpp.ProcessInEbThreadUnsafe
  map<string, i64> getCounters();

  /**
   * Gets a subset of counters which match a
   * Perl Compatible Regular Expression for this service
   */
  @cpp.ProcessInEbThreadUnsafe
  map<string, i64> getRegexCounters(1: string regex);

  /**
   * Get counter values for a specific list of keys.  Returns a map from
   * key to counter value; if a requested counter doesn't exist, it won't
   * be in the returned map.
   */
  @cpp.ProcessInEbThreadUnsafe
  map<string, i64> getSelectedCounters(1: list<string> keys);

  /**
   * Gets the value of a single counter
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  i64 getCounter(1: string key);

  /**
   * Gets the exported string values for this service
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  map<string, string> getExportedValues();

  /**
   * Get exported strings for a specific list of keys.  Returns a map from
   * key to string value; if a requested key doesn't exist, it won't
   * be in the returned map.
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  map<string, string> getSelectedExportedValues(1: list<string> keys);

  /**
   * Gets a subset of exported values which match a
   * Perl Compatible Regular Expression for this service
   */
  map<string, string> getRegexExportedValues(1: string regex);

  /**
   * Gets the value of a single exported string
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  string getExportedValue(1: string key);

  /**
   * Sets an option
   */
  void setOption(1: string key, 2: string value);

  /**
   * Gets an option
   */
  string getOption(1: string key);

  /**
   * Gets all options
   */
  map<string, string> getOptions();

  /**
   * Returns the unix time that the server has been running since
   */
  @thrift.Priority{level = thrift.RpcPriority.IMPORTANT}
  i64 aliveSince();
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.Runtime.Serialization;

namespace InteractiveSmartlogVSExtension
{
    [DataContract]
    class CommandExecutionResult
    {
        [DataMember]
        public string Url { get; set; }

        [DataMember]
        public string Port { get; set; }

        [DataMember]
        public string Token { get; set; }

        [DataMember]
        public string Pid { get; set; }

        [DataMember]
        public bool WasServerReused { get; set; }

        [DataMember]
        public string LogFileLocation { get; set; }

        [DataMember]
        public string Command { get; set; }
    }
}

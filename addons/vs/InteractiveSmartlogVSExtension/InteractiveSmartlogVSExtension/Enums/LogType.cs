/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System.Runtime.Serialization;

namespace InteractiveSmartlogVSExtension.Enums
{
    [DataContract]
    public enum LogType
    {
        [DataMember(Name = "Information")]
        Information,

        [DataMember(Name = "Error")]
        Error
    }
}

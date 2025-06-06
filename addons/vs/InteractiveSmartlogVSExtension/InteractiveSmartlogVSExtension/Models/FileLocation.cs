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
    public class FileLocation
    {
        [System.Text.Json.Serialization.JsonPropertyName("filePath")]
        [DataMember]
        public string FilePath { get; set; }

        [System.Text.Json.Serialization.JsonPropertyName("line")]
        [DataMember]
        public int Line { get; set; }

        [System.Text.Json.Serialization.JsonPropertyName("col")]
        [DataMember]
        public int Col { get; set; }
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.Runtime.Serialization;

namespace InteractiveSmartlogVSExtension
{
    public class DiffData
    {
        [System.Text.Json.Serialization.JsonPropertyName("filePath")]
        public string FilePath { get; set; }

        [System.Text.Json.Serialization.JsonPropertyName("comparison")]
        public Comparison Comparison { get; set; }
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.Runtime.Serialization;

namespace InteractiveSmartlogVSExtension
{
    public class Comparison
    {
        [System.Text.Json.Serialization.JsonPropertyName("type")]
        [System.Text.Json.Serialization.JsonConverter(typeof(System.Text.Json.Serialization.JsonStringEnumConverter))]
        public DiffType Type { get; set; } // Update property type to enum
    }
}

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
    public enum ErrorCodes
    {
        [DataMember(Name = "Package Initialization Failed")]
        PackageInitializationFailed,

        [DataMember(Name = "WebView Initialization Failed")]
        WebViewInitializationFailed,

        [DataMember(Name = "WebView Directory Creation Failed")]
        WebViewDirectoryCreationFailed,

        [DataMember(Name = "WebView Environment Creation Failed")]
        WebViewEnvironmentCreationFailed,

        [DataMember(Name = "SlWeb Failed")]
        SlWebFailed,

        [DataMember(Name = "Invalid File Location")]
        InvalidFileLocation,

        [DataMember(Name = "Invalid Diff Data")]
        InvalidDiffData,

        [DataMember(Name = "Diff View Rendering Failed")]
        DiffViewRenderingFailed,

        [DataMember(Name = "File Not Found")]
        FileNotFound,

        [DataMember(Name = "File Open Failed")]
        FileOpenFailed,

        [DataMember(Name = "SL cat failed")]
        SlCatFailed,
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System;
using System.Collections.Generic;
using System.IO;

namespace InteractiveSmartlogVSExtension
{
    public static class Constants
    {
        /// <summary>
        /// Scribe table name.
        /// </summary>
        public static string ScribeCategory = "perfpipe_vs_isl_view"; // fb-only

        /// <summary>
        /// Scribe table tags.
        /// </summary>
        public const string ISLExtensionName = "fb-islvsextension";

        /// <summary>
        /// Maximum number of attempts to initialize the ISL tool window.
        /// </summary>
        public const int MaxToolWindowInitializationAttempts = 5; // Maximum number of attempts
        public const int ToolWindowInitializationRetryDelayMs = 1000; // Delay between attempts

        /// <summary>
        /// Maximum number of attempts to load the webview url.
        /// </summary>
        public const int MaxSlOperationRetries = 3;
        public const int SlOperationRetryDelayMs = 1000;
        // Process timeout ensures that the sl.exe process does not hang indefinitely.
        // It's crucial for preventing resource leaks and ensuring responsiveness.
        public const int SlOperationProcessTimeoutMs = 5000;
        // Operation timeout ensures that the entire operation (including retries) does not exceed a certain duration.
        // This is important for maintaining overall system responsiveness and preventing prolonged stalls.
        public const int OverallSlOperationTimeoutMs = 30000;
        // Check if the cached URL is fresh based on the staleness threshold
        public const int StalenessThresholdMinutes = 5;

        public const string SlInstallationUrl = "https://sapling-scm.com/docs/introduction/installation";

        /// <summary>
        /// Command ID.
        /// </summary>
        public const int ISLToolWindowCommandId = 0x0100;
        public const int ReloadCommandId = 0x0101;
        public const int DiffUncommittedChangesCommandId = 0x0102;
        public const int DiffStackChangesCommandId = 0x0103;
        public const int DiffHeadChangesCommandId = 0x0104;
        public const int RevertUncommittedChangesCommandId = 0x0105;

        /// <summary>
        /// Command menu group (command set GUID).
        /// </summary>
        public static readonly Guid ToolWindowViewCommandSet = new Guid("ea6d6b22-8b69-45e5-a008-60e544350cbf");
        public static readonly Guid ToolsMenuCommandSet = new Guid("e403bf7c-0df0-4f85-8c38-0c100aec6d36");
        public static readonly Guid ContextMenuCommandSet = new Guid("de9ba81f-a1bf-456a-91af-3d495add8add");

        /// <summary>
        /// Error messages.
        /// </summary>
        public static string NoRepoErrorTitle = "Invalid Repository Detected.";
        public static string NoRepoErrorDetail = "No valid Sapling repository is detected in the current workspace.";
        public static string NoRepoErrorRemediation = "Please clone or init a repository and reload view via Tools -> Reload ISL.";

        public static string NoRepoMountedErrorTitle = "Repository is not mounted.";
        public static string NoRepoMountedErrorDetail = $"Check if EdenFS is running and ensure repository is mounted.";
        public static string NoRepoMountedErrorRemediation = "Try running `edenfsctl doctor` and reload view via Tools -> Reload ISL.";


        public static string SaplingExeNotFoundTitle = "Sapling (sl.exe) Not Found.";
        public static string SaplingExeNotFoundDetail = "The Sapling command-line tool sl.exe was not found on your system PATH.";
        public static string SaplingExeNotFoundRemediation = $"Please install Sapling from {SlInstallationUrl} and ensure sl.exe is accessible from your command line. After installation, restart Visual Studio and try loading ISL view again.";

        public static readonly HashSet<ErrorCodes> ErrorCodesThatShouldCloseToolWindow = new HashSet<ErrorCodes>
        {
            ErrorCodes.PackageInitializationFailed,
            ErrorCodes.WebViewInitializationFailed,
            ErrorCodes.WebViewDirectoryCreationFailed,
            ErrorCodes.WebViewEnvironmentCreationFailed,
            ErrorCodes.SlWebFailed
        };
    }
}

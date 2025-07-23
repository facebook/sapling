/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using InteractiveSmartlogVSExtension;
using InteractiveSmartlogVSExtension.Helpers;
using Meta.VisualStudio.ScubaLogger;
using Meta.Windows.ScubaUtility;

namespace InteractiveSmartlogVSExtension.Telemetry
{
    [ScubaTable("perfpipe_vs_isl_view")]
    public class ISLLogEntry : LogEntryBase
    {
        [ScubaProperty("session_id")]
        public string SessionId { get; set; }

        [ScubaProperty("ide_name")]
        public string IdeName { get; set; }

        [ScubaProperty("ide_version")]
        public string IdeVersion { get; set; }

        [ScubaProperty("action")]
        public string Action { get; set; }

        [ScubaProperty("log_type")]
        public string LogType { get; set; }

        [ScubaProperty("error_code")]
        public string ErrorCode { get; set; }

        [ScubaProperty("error_message")]
        public string ErrorMessage { get; set; }

        public ISLLogEntry(
            string sessionId,
            string ideName,
            string ideVersion,
            string action,
            string logType,
            string errorCode = null,
            string errorMessage = null)
            : base()
        {
            SessionId = sessionId;
            IdeName = ideName;
            IdeVersion = ideVersion;
            Action = action;
            LogType = logType;
            ErrorCode = errorCode;
            ErrorMessage = errorMessage;

            // Set the plugin name and version for the base class
            PluginName = Constants.ISLExtensionName;
            PluginVersion = CommonHelper.GetExtensionVersion();
        }
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Collections.Generic;
using System.Diagnostics;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Helpers;
using Newtonsoft.Json;

namespace InteractiveSmartlogVSExtension
{
    public partial class TelemetryLogger
    {
        private static readonly string IntTag = "int";
        private static readonly string NormalTag = "normal";

        private static string _sessionId;
        private string _userName;
        private string _hostName;
        private string _ideName;
        private string _ideVersion;
        private string _extensionName;
        private string _extensionVersion;
        private long _time;

        public TelemetryLogger()
        {
            _time = CommonHelper.GetTime();
            _userName = CommonHelper.GetUserName();
            _hostName = CommonHelper.GetHostName();
            _sessionId = CommonHelper.WindowId.Value;
            _ideName = CommonHelper.GetIDEName();
            _ideVersion = CommonHelper.GetIDEVersion();
            _extensionName = Constants.ISLExtensionName;
            _extensionVersion = CommonHelper.GetExtensionVersion();
        }

        public void logInfo(ActionType action)
        {
            Dictionary<string, string> strings = getBaseString();
            strings["action"] = action.ToString();
            strings["log_type"] = LogType.Information.ToString();

            this.writeToScuba(strings);
        }
        public void logError(ActionType action, ErrorCodes errorCode, string errorMessage = "")
        {
            Dictionary<string, string> strings = getBaseString();
            strings["action"] = action.ToString();
            strings["error_code"] = errorCode.ToString();
            strings["error_message"] = errorMessage;
            strings["log_type"] = LogType.Error.ToString();

            // log the error to scribe
            this.writeToScuba(strings);
        }

        private void writeToScuba(Dictionary<string, string> strings)
        {
            // Integer data to scuba table.
            var ints = new Dictionary<string, long>();
            ints["time"] = _time;
            var intsJson = JsonConvert.SerializeObject(ints);

            // Normal Data to scuba table.
            var stringsJson = JsonConvert.SerializeObject(strings);

            string jsonData = $"{{\"{IntTag}\":{intsJson},\"{NormalTag}\":{stringsJson}}}";

            var res = ScribeToolManager.Execute("scribe_cat",
               $"{Constants.ScribeCategory} \"{jsonData.Replace("\"", "\\\"")}\"");

            if (res.ExitCode != 0)
            {
                _ = LoggingHelper.WriteAsync($"Error writing ISL metrics to scuba: {res.Stderr}");
            }
            else
            {
                _ = LoggingHelper.WriteAsync($"Successfully write ISL metrics to scuba: {res.Stdout}");
            }
        }

        private Dictionary<string, string> getBaseString()
        {
            var strings = new Dictionary<string, string>();
            strings["session_id"] = _sessionId;
            strings["username"] = _userName;
            strings["hostname"] = _hostName;
            strings["ide_name"] = _ideName;
            strings["ide_version"] = _ideVersion;
            strings["extension_name"] = _extensionName;
            strings["extension_version"] = _extensionVersion;
            return strings;
        }
    }
}

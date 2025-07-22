/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Diagnostics;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Helpers;
using InteractiveSmartlogVSExtension.Telemetry;
using Meta.VisualStudio.ScubaLogger;

namespace InteractiveSmartlogVSExtension
{
    public partial class TelemetryLogger
    {
        private static readonly Logger<ISLLogEntry> _logger = new Logger<ISLLogEntry>();
        private static string _sessionId;
        private string _ideName;
        private string _ideVersion;

        public TelemetryLogger()
        {
            _sessionId = CommonHelper.WindowId.Value;
            _ideName = CommonHelper.GetIDEName();
            _ideVersion = CommonHelper.GetIDEVersion();
        }

        public void logInfo(ActionType action)
        {
            _logger.Log(new ISLLogEntry(
                _sessionId,
                _ideName,
                _ideVersion,
                action.ToString(),
                LogType.Information.ToString()));
        }

        public void logError(ActionType action, ErrorCodes errorCode, string errorMessage = "")
        {
            _logger.Log(new ISLLogEntry(
                _sessionId,
                _ideName,
                _ideVersion,
                action.ToString(),
                LogType.Error.ToString(),
                errorCode.ToString(),
                errorMessage));
        }
    }
}

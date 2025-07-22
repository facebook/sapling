/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Collections.Generic;
using InteractiveSmartlogVSExtension;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Telemetry;
using Meta.VisualStudio.Shared.ScubaLogger;
using Meta.VisualStudio.Shared.ScubaLogger.Interfaces;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Moq;

namespace InteractiveSmartlogVSExtension.Tests
{
    [TestClass]
    public class TelemetryLoggerTests
    {
        private TelemetryLogger _telemetryLogger;
        private Mock<IScubaLogger> _mockScubaLogger;

        [TestInitialize]
        public void Initialize()
        {
            _mockScubaLogger = new Mock<IScubaLogger>();
            _telemetryLogger = new TelemetryLogger(_mockScubaLogger.Object);
        }

        [TestMethod]
        public void LogInfo_ShouldLogWithCorrectProperties()
        {
            // Arrange
            var actionType = ActionType.RenderISLView;
            var expectedProperties = new Dictionary<string, object>
            {
                { "Action", actionType.ToString() },
                { "LogType", LogType.Information.ToString() }
            };

            // Act
            _telemetryLogger.logInfo(actionType);

            // Assert
            _mockScubaLogger.Verify(l => l.LogEvent(
                It.Is<string>(s => s == "ISL"),
                It.Is<Dictionary<string, object>>(d =>
                    d.ContainsKey("Action") && d["Action"].ToString() == actionType.ToString() &&
                    d.ContainsKey("LogType") && d["LogType"].ToString() == LogType.Information.ToString() &&
                    (!d.ContainsKey("ErrorCode") || d["ErrorCode"] == null) &&
                    (!d.ContainsKey("ErrorMessage") || d["ErrorMessage"] == null)
                )
            ), Times.Once);
        }

        [TestMethod]
        public void LogError_ShouldLogWithCorrectProperties()
        {
            // Arrange
            var actionType = ActionType.OpenFile;
            var errorCode = ErrorCodes.FileNotFound;
            var errorMessage = "Test error message";

            // Act
            _telemetryLogger.logError(actionType, errorCode, errorMessage);

            // Assert
            _mockScubaLogger.Verify(l => l.LogEvent(
                It.Is<string>(s => s == "ISL"),
                It.Is<Dictionary<string, object>>(d =>
                    d.ContainsKey("Action") && d["Action"].ToString() == actionType.ToString() &&
                    d.ContainsKey("LogType") && d["LogType"].ToString() == LogType.Error.ToString() &&
                    d.ContainsKey("ErrorCode") && d["ErrorCode"].ToString() == errorCode.ToString() &&
                    d.ContainsKey("ErrorMessage") && d["ErrorMessage"].ToString() == errorMessage
                )
            ), Times.Once);
        }
    }
}

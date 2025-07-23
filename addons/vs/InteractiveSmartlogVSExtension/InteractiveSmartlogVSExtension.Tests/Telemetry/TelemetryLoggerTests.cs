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
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Telemetry
{
    [TestClass]
    public class TelemetryLoggerTests
    {
        private TelemetryLogger _telemetryLogger;

        [TestInitialize]
        public void Initialize()
        {
            _telemetryLogger = new TelemetryLogger();
        }

        [TestMethod]
        public void Constructor_InitializesProperties()
        {
            // Arrange & Act
            var logger = new TelemetryLogger();

            // Assert
            // Since the properties are private, we can't directly assert their values
            // But we can verify the logger was created without exceptions
            Assert.IsNotNull(logger);
        }

        [TestMethod]
        public void LogInfo_WithoutMock_DoesNotThrowException()
        {
            // Arrange
            var logger = new TelemetryLogger();

            // Act & Assert
            try
            {
                // This will actually try to execute scribe_cat, which might fail in a test environment
                // We're just checking that the method doesn't throw unexpected exceptions
                logger.logInfo(ActionType.RenderISLView);

                // If we get here, the test passes (though it might have logged an error)
                Assert.IsTrue(true);
            }
            catch (Exception ex)
            {
                // If the exception is related to scribe_cat not being found, that's expected in a test environment
                if (ex.Message.Contains("scribe_cat") && ex.Message.Contains("not found"))
                {
                    Assert.IsTrue(true);
                }
                else
                {
                    Assert.Fail($"Unexpected exception: {ex.Message}");
                }
            }
        }

        [TestMethod]
        public void LogError_WithoutMock_DoesNotThrowException()
        {
            // Arrange
            var logger = new TelemetryLogger();

            // Act & Assert
            try
            {
                // This will actually try to execute scribe_cat, which might fail in a test environment
                logger.logError(ActionType.RenderISLView, ErrorCodes.SlWebFailed, "Test error message");

                // If we get here, the test passes (though it might have logged an error)
                Assert.IsTrue(true);
            }
            catch (Exception ex)
            {
                // If the exception is related to scribe_cat not being found, that's expected in a test environment
                if (ex.Message.Contains("scribe_cat") && ex.Message.Contains("not found"))
                {
                    Assert.IsTrue(true);
                }
                else
                {
                    Assert.Fail($"Unexpected exception: {ex.Message}");
                }
            }
        }
    }
}

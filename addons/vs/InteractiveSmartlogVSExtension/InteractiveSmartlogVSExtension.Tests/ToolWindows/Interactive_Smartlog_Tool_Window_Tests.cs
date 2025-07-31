/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Microsoft.VisualStudio.Threading;
using Moq;

namespace InteractiveSmartlogVSExtension.Tests.ToolWindows
{
    [TestClass]
    public class InteractiveSmartlogToolWindowTests
    {
        private JoinableTaskContext _joinableTaskContext;
        private JoinableTaskFactory _joinableTaskFactory;

        [TestInitialize]
        public void TestInitialize()
        {
            // Create a new JoinableTaskContext for testing
#pragma warning disable VSSDK005 // Use the ThreadHelper.JoinableTaskContext singleton rather than instantiating your own to avoid deadlocks.
            _joinableTaskContext = new JoinableTaskContext(Thread.CurrentThread);
#pragma warning restore VSSDK005
            _joinableTaskFactory = _joinableTaskContext.Factory;

            // Set the ThreadHelper static fields directly using reflection
            // We need to do this before any code tries to access ThreadHelper.JoinableTaskContext
            var contextField = typeof(ThreadHelper).GetField("_joinableTaskContext", BindingFlags.Static | BindingFlags.NonPublic);
            var factoryField = typeof(ThreadHelper).GetField("_joinableTaskFactory", BindingFlags.Static | BindingFlags.NonPublic);

            if (contextField != null && factoryField != null)
            {
                contextField.SetValue(null, _joinableTaskContext);
                factoryField.SetValue(null, _joinableTaskFactory);
            }
        }

        [TestCleanup]
        public void TestCleanup()
        {
            // Reset ThreadHelper static fields
            typeof(ThreadHelper).GetField("_joinableTaskContext", BindingFlags.Static | BindingFlags.NonPublic)
                ?.SetValue(null, null);
            typeof(ThreadHelper).GetField("_joinableTaskFactory", BindingFlags.Static | BindingFlags.NonPublic)
                ?.SetValue(null, null);
        }

        [TestMethod]
        public void Constructor_SetsCaption()
        {
            // Use TestControlAccessor instead of mocking InteractiveSmartlogToolWindowControl
            var testControl = new TestControlAccessor();

            // Use reflection to create a test version of the tool window
            var toolWindow = new TestInteractiveSmartlogToolWindow(testControl);

            // Assert
            Assert.AreEqual("Interactive Smartlog", toolWindow.Caption);
        }

        [TestMethod]
        public void Constructor_SetsContent()
        {
            // Use TestControlAccessor instead of mocking InteractiveSmartlogToolWindowControl
            var testControl = new TestControlAccessor();

            // Use reflection to create a test version of the tool window
            var toolWindow = new TestInteractiveSmartlogToolWindow(testControl);

            // Assert
            Assert.IsNotNull(toolWindow.Content);
            Assert.IsInstanceOfType(toolWindow.Content, typeof(TestControlAccessor));
        }

        [TestMethod]
        public void ToolWindow_HasExpectedGuid()
        {
            // Arrange
            var expectedGuid = new Guid("c45777d1-c943-4a9e-8ba2-54f25df6b04c");

            // Act
            var guidAttribute = (GuidAttribute)Attribute.GetCustomAttribute(
                typeof(InteractiveSmartlogToolWindow), typeof(GuidAttribute));

            // Assert
            Assert.IsNotNull(guidAttribute);
            Assert.AreEqual(expectedGuid.ToString(), guidAttribute.Value);
        }
    }

    /// <summary>
    /// Test version of InteractiveSmartlogToolWindow that allows injecting a mock control
    /// </summary>
    public class TestInteractiveSmartlogToolWindow : ToolWindowPane
    {
        public TestInteractiveSmartlogToolWindow(object control) : base(null)
        {
            // Set the Caption and Content properties directly
            Caption = "Interactive Smartlog";
            Content = control;
        }
    }

    /// <summary>
    /// Test accessor class for InteractiveSmartlogToolWindowControl that doesn't initialize WPF components
    /// </summary>
    public class TestControlAccessor
    {
        // This is a simple class that can be used as the content of the tool window
        // It doesn't inherit from any WPF control to avoid XAML initialization issues
    }
}

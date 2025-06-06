/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System;
using System.ComponentModel;
using System.Reflection;

namespace InteractiveSmartlogVSExtension
{
    public class DiffToolConverter : EnumConverter
    {
        public DiffToolConverter(Type type) : base(type) { }
        public override object ConvertTo(ITypeDescriptorContext context, System.Globalization.CultureInfo culture, object value, Type destinationType)
        {
            if (destinationType == typeof(string) && value is Enum)
            {
                var fieldInfo = value.GetType().GetField(value.ToString());
                var descriptionAttribute = fieldInfo.GetCustomAttribute<DescriptionAttribute>();
                return descriptionAttribute != null ? descriptionAttribute.Description : value.ToString();
            }
            return base.ConvertTo(context, culture, value, destinationType);
        }
        public override object ConvertFrom(ITypeDescriptorContext context, System.Globalization.CultureInfo culture, object value)
        {
            if (value is string)
            {
                foreach (var field in EnumType.GetFields())
                {
                    var descriptionAttribute = field.GetCustomAttribute<DescriptionAttribute>();
                    if (descriptionAttribute != null && descriptionAttribute.Description == (string)value)
                    {
                        return Enum.Parse(EnumType, field.Name);
                    }
                }
            }
            try
            {
                return base.ConvertFrom(context, culture, value);
            }
            catch
            {
                return DiffTool.VisualStudio;
            }
        }
    }
}


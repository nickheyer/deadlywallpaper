using Lively.Common.Helpers.Hardware;
using System;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lively.Common.Helpers
{
    public static class LogUtilWindows
    {
        /// <summary>
        /// Get hardware information
        /// </summary>
        public static string GetHardwareInfo()
        {
            var arch = Environment.Is64BitProcess ? "x64" : "x86";
            var container = PackageUtil.IsRunningAsPackaged ? "desktop-bridge" : "desktop-native";
            return $"\nLively v{Assembly.GetEntryAssembly().GetName().Version} {arch} {container} {CultureInfo.CurrentUICulture.Name}" +
                $"\n{SystemInfo.GetOSInfo()}\n{SystemInfo.GetCpuInfo()}\n{SystemInfo.GetGpuInfo()}\n";
        }

        /// <summary>
        /// Return string representation of win32 error.
        /// </summary>
        /// <param name="message"></param>
        /// <param name="memberName"></param>
        /// <param name="fileName"></param>
        /// <param name="lineNumber"></param>
        /// <returns></returns>
        public static string GetWin32Error(string message,
            [CallerMemberName] string memberName = "",
            [CallerFilePath] string fileName = "",
            [CallerLineNumber] int lineNumber = 0)
        {
            int err = Marshal.GetLastWin32Error();
            return $"HRESULT: {err}, {message} at\n{fileName} ({lineNumber})\n{memberName}";
        }
    }
}

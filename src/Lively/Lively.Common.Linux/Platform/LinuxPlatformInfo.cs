using Lively.Common.Services;

namespace Lively.Common.Linux.Platform
{
    /// <summary>
    /// Platform facts for the Linux build. Lively is distributed as a plain (unpackaged) install on Linux.
    /// </summary>
    public sealed class LinuxPlatformInfo : IPlatformInfo
    {
        public bool IsPackaged => false;
    }
}

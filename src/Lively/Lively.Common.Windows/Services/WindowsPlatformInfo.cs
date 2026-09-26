using Lively.Common.Helpers;

namespace Lively.Common.Services
{
    public class WindowsPlatformInfo : IPlatformInfo
    {
        public bool IsPackaged => PackageUtil.IsRunningAsPackaged;
    }
}

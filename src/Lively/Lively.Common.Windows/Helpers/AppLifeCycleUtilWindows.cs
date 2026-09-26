using System.IO;
using System.Linq;
using static Lively.Common.Constants;

namespace Lively.Common.Helpers
{
    public static class AppLifeCycleUtilWindows
    {
        public static bool IsNamedPipeExists(string pipeName) => 
            Directory.GetFiles("\\\\.\\pipe\\").Any(f => f.Equals("\\\\.\\pipe\\" + pipeName));

        public static LivelyAppVer GetRunningLivelyAppVer()
        {
            if (AppLifeCycleUtil.IsAppMutexRunning(SingleInstance.UniqueAppName))
            {
                return IsNamedPipeExists(SingleInstance.GrpcPipeServerName) ? LivelyAppVer.v2 : LivelyAppVer.v1;
            }
            return LivelyAppVer.nil;
        }

        public enum LivelyAppVer
        {
            nil,
            v1,
            v2
        }
    }
}

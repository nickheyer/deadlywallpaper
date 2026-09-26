using Lively.Common.Helpers;
using Lively.Common.Services;
using Lively.Helpers;
using System.Threading.Tasks;
using Windows.ApplicationModel;

namespace Lively.Services
{
    public class WindowsStartupService : IStartupService
    {
        public async Task<bool> TrySetStartupAsync(bool enabled)
        {
            try
            {
                if (PackageUtil.IsRunningAsPackaged)
                {
                    var state = await WindowsStartup.SetStartupTask(enabled);
                    var isEnabled = state == StartupTaskState.Enabled || state == StartupTaskState.EnabledByPolicy;
                    return enabled == isEnabled;
                }
                WindowsStartup.SetStartupRegistry(enabled);
                return true;
            }
            catch
            {
                return false;
            }
        }
    }
}

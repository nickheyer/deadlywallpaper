using Google.Protobuf.WellKnownTypes;
using Grpc.Core;
using Lively.Common.Services;
using Lively.Grpc.Common.Proto.Update;
using Lively.RPC;
using System.Diagnostics;
using System.Threading.Tasks;

namespace Lively.Core.Linux.RPC
{
    /// <summary>
    /// On Windows "start update" runs the downloaded installer. Linux installs come from the
    /// package manager or `make install`, so updating means opening the release page.
    /// </summary>
    public sealed class LinuxAppUpdateServer : AppUpdateServer
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly IAppUpdaterService updater;

        public LinuxAppUpdateServer(IAppUpdaterService updater, IDownloadService downloader, IDispatcherService dispatcher,
            IWindowService windowService, IResourceService i18n, IAppLifetimeService appLifetime, IPlatformInfo platformInfo)
            : base(updater, downloader, dispatcher, windowService, i18n, appLifetime, platformInfo)
        {
            this.updater = updater;
        }

        public override Task<Empty> StartUpdate(Empty _, ServerCallContext context)
        {
            var url = updater.LastCheckUri?.OriginalString;
            if (string.IsNullOrEmpty(url))
                throw new RpcException(new Status(StatusCode.FailedPrecondition, "No release has been checked yet."));
            Logger.Info($"Opening release page for update: {url}");
            Process.Start(new ProcessStartInfo { FileName = "xdg-open", ArgumentList = { url }, UseShellExecute = false });
            return Task.FromResult(new Empty());
        }

        public override Task<Empty> SwitchReleaseChannel(SwitchReleaseChannelRequest request, ServerCallContext context)
        {
            throw new RpcException(new Status(StatusCode.Unimplemented, "Switching release channels reinstalls the app with the Windows installer; on Linux install the wanted build with `make install` instead."));
        }
    }
}

using Lively.Common.Helpers;
using Lively.Common.Services;
using Lively.Models.Services;
using Octokit;
using System;
using System.Threading.Tasks;
using Timer = System.Timers.Timer;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// Update checks against this port's own GitHub releases. On Linux an "update" is a link to the
    /// release page (installs come from the package manager or `make install`), so the URL is the
    /// release's HTML page and the file name is its tag.
    /// </summary>
    public sealed class LinuxGithubUpdaterService : IAppUpdaterService
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        public const string RepositoryOwner = "nickheyer";
        public const string RepositoryName = "deadlywallpaper";

        private readonly int fetchDelayError = 30 * 60 * 1000;
        private readonly int fetchDelayRepeat = 12 * 60 * 60 * 1000;
        private readonly Timer retryTimer = new Timer { Interval = 5 * 60 * 1000 };

        public AppUpdateStatus Status { get; private set; } = AppUpdateStatus.notchecked;
        public DateTime LastCheckTime { get; private set; } = DateTime.MinValue;
        public Version LastCheckVersion { get; private set; } = new Version(0, 0, 0, 0);
        public Uri LastCheckUri { get; private set; }
        public string LastCheckFileName { get; private set; }

        public event EventHandler<AppUpdaterEventArgs> UpdateChecked;

        public LinuxGithubUpdaterService()
        {
            retryTimer.Elapsed += (s, e) =>
            {
                if ((DateTime.Now - LastCheckTime).TotalMilliseconds > (Status != AppUpdateStatus.error ? fetchDelayRepeat : fetchDelayError))
                    _ = CheckUpdate(0);
            };
        }

        public void Start() => retryTimer.Start();

        public void Stop()
        {
            if (retryTimer.Enabled)
                retryTimer.Stop();
        }

        public async Task<AppUpdateStatus> CheckUpdate(int fetchDelay)
        {
            if (BuildInfoUtil.IsDebugBuild())
                return AppUpdateStatus.notchecked;

            try
            {
                await Task.Delay(fetchDelay);
                var (url, fileName, version) = await GetLatestRelease(false);
                var compare = GithubUtil.CompareAssemblyVersion(version);
                Status = compare > 0 ? AppUpdateStatus.available : compare < 0 ? AppUpdateStatus.invalid : AppUpdateStatus.uptodate;
                LastCheckUri = url;
                LastCheckVersion = version;
                LastCheckFileName = fileName;
            }
            catch (Exception e)
            {
                Logger.Warn($"Update check failed: {e.Message}");
                Status = AppUpdateStatus.error;
            }
            LastCheckTime = DateTime.Now;
            UpdateChecked?.Invoke(this, new AppUpdaterEventArgs(Status, LastCheckVersion, LastCheckTime, LastCheckUri, LastCheckFileName));
            return Status;
        }

        public async Task<(Uri Url, string FileName, Version AppVersion)> GetLatestRelease(bool isBeta)
        {
            var client = new GitHubClient(new ProductHeaderValue(RepositoryName));
            var releases = await client.Repository.Release.GetAll(RepositoryOwner, RepositoryName);
            Release latest = null;
            foreach (var release in releases)
            {
                if (release.Draft || (release.Prerelease && !isBeta))
                    continue;
                latest = release;
                break;
            }
            if (latest == null)
                throw new InvalidOperationException($"No {(isBeta ? "pre-" : "")}release published at github.com/{RepositoryOwner}/{RepositoryName}.");

            var version = GithubUtil.GetVersion(latest);
            return (new Uri(latest.HtmlUrl), latest.TagName, version);
        }
    }
}

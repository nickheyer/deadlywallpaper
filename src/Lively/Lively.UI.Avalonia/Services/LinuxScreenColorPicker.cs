using Avalonia.Media;
using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.UI.Avalonia.Services
{
    [DBusInterface("org.freedesktop.portal.Screenshot")]
    public interface IScreenshotPortal : IDBusObject
    {
        Task<ObjectPath> PickColorAsync(string parentWindow, IDictionary<string, object> options);
    }

    [DBusInterface("org.freedesktop.portal.Request")]
    public interface IPortalRequest : IDBusObject
    {
        Task<IDisposable> WatchResponseAsync(Action<(uint response, IDictionary<string, object> results)> handler, Action<Exception> onError = null);
        Task CloseAsync();
    }

    /// <summary>
    /// Picks a colour from the screen through the XDG desktop portal (works on Wayland and X11 sessions that run xdg-desktop-portal).
    /// </summary>
    public sealed class LinuxScreenColorPicker
    {
        private const string PortalService = "org.freedesktop.portal.Desktop";
        private static readonly ObjectPath PortalPath = new ObjectPath("/org/freedesktop/portal/desktop");

        /// <summary>
        /// Shows the portal picker and returns the chosen colour, or null when the user cancelled.
        /// </summary>
        public async Task<Color?> PickAsync(CancellationToken cancellationToken = default)
        {
            using var connection = new Connection(Address.Session);
            var connectionInfo = await connection.ConnectAsync();

            var sender = connectionInfo.LocalName.TrimStart(':').Replace('.', '_');
            var token = "lively" + Guid.NewGuid().ToString("N");
            var requestPath = new ObjectPath($"/org/freedesktop/portal/desktop/request/{sender}/{token}");

            var completion = new TaskCompletionSource<(uint response, IDictionary<string, object> results)>(TaskCreationOptions.RunContinuationsAsynchronously);
            var request = connection.CreateProxy<IPortalRequest>(PortalService, requestPath);
            using var subscription = await request.WatchResponseAsync(result => completion.TrySetResult(result), ex => completion.TrySetException(ex));

            var portal = connection.CreateProxy<IScreenshotPortal>(PortalService, PortalPath);
            var handle = await portal.PickColorAsync(string.Empty, new Dictionary<string, object> { ["handle_token"] = token });
            if (handle != requestPath)
            {
                // Older portals return a different request path, subscribe to the one they chose.
                subscription.Dispose();
                request = connection.CreateProxy<IPortalRequest>(PortalService, handle);
                using var lateSubscription = await request.WatchResponseAsync(result => completion.TrySetResult(result), ex => completion.TrySetException(ex));
                return await WaitForColorAsync(completion, request, cancellationToken);
            }

            return await WaitForColorAsync(completion, request, cancellationToken);
        }

        private static async Task<Color?> WaitForColorAsync(TaskCompletionSource<(uint response, IDictionary<string, object> results)> completion, IPortalRequest request, CancellationToken cancellationToken)
        {
            using var registration = cancellationToken.Register(() =>
            {
                completion.TrySetCanceled(cancellationToken);
                _ = request.CloseAsync();
            });

            var (response, results) = await completion.Task;
            if (response != 0 || results == null || !results.TryGetValue("color", out var value))
                return null;

            if (value is ValueTuple<double, double, double> rgb)
                return Color.FromRgb(ToByte(rgb.Item1), ToByte(rgb.Item2), ToByte(rgb.Item3));

            throw new InvalidOperationException($"Portal returned an unexpected colour payload: {value?.GetType().Name ?? "null"}");
        }

        private static byte ToByte(double channel) => (byte)Math.Clamp(Math.Round(channel * 255), 0, 255);
    }
}

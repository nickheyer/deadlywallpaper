using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.Linq;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Core.Linux.Plasma
{
    [DBusInterface("org.kde.PlasmaShell")]
    public interface IPlasmaShell : IDBusObject
    {
        Task<string> evaluateScriptAsync(string script);
    }

    /// <summary>A Plasma desktop containment (one per screen) as seen through the scripting API.</summary>
    public sealed class PlasmaDesktop
    {
        public int Id { get; set; }
        public int Screen { get; set; }
        public Rectangle Geometry { get; set; }
        public string WallpaperPlugin { get; set; }
    }

    /// <summary>A Plasma panel as seen through the scripting API (panels()).</summary>
    public sealed class PlasmaPanel
    {
        public int Id { get; set; }
        public int Screen { get; set; }
        /// <summary>top, bottom, left or right.</summary>
        public string Location { get; set; }
        /// <summary>Thickness in logical pixels (the scripting API's "height", whatever the orientation).</summary>
        public int Thickness { get; set; }
        /// <summary>none (always visible, reserves space), autohide, dodgewindows, windowsbelow, windowsgobelow.</summary>
        public string Hiding { get; set; }

        /// <summary>Only an always-visible panel takes space away from maximized windows.</summary>
        public bool ReservesSpace => string.Equals(Hiding, "none", StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Drives plasmashell through org.kde.PlasmaShell.evaluateScript to switch desktops to the
    /// Lively wallpaper plugin and write its configuration (PROTOCOL.md section 7).
    /// </summary>
    public sealed class PlasmaShellScripting
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        public const string PluginId = "com.lively.wallpaper";
        // Bus name is lowercase; the interface on /PlasmaShell is org.kde.PlasmaShell.
        private const string ServiceName = "org.kde.plasmashell";
        private static readonly ObjectPath ObjectPath = new ObjectPath("/PlasmaShell");

        private readonly Connection connection;

        public PlasmaShellScripting(Connection sessionBus)
        {
            connection = sessionBus;
        }

        public async Task<bool> IsAvailableAsync()
        {
            return await connection.IsServiceActiveAsync(ServiceName);
        }

        public async Task<string> EvaluateAsync(string script)
        {
            var shell = connection.CreateProxy<IPlasmaShell>(ServiceName, ObjectPath);
            return await shell.evaluateScriptAsync(script);
        }

        public async Task<List<PlasmaDesktop>> ListDesktopsAsync()
        {
            const string script = @"
var out = [];
var ds = desktopsForActivity(currentActivity());
for (var i = 0; i < ds.length; i++) {
  var d = ds[i];
  if (d.screen < 0) continue;
  var g = screenGeometry(d.screen);
  out.push({ id: d.id, screen: d.screen, x: g.x, y: g.y, width: g.width, height: g.height, plugin: d.wallpaperPlugin });
}
print(JSON.stringify(out));";
            var json = await EvaluateAsync(script);
            var result = new List<PlasmaDesktop>();
            JArray array;
            try
            {
                array = JArray.Parse(json.Trim());
            }
            catch (JsonException ex)
            {
                throw new InvalidOperationException($"plasmashell returned unexpected output for the desktop list: {json}", ex);
            }
            foreach (var item in array)
            {
                result.Add(new PlasmaDesktop
                {
                    Id = (int)item["id"],
                    Screen = (int)item["screen"],
                    Geometry = new Rectangle((int)item["x"], (int)item["y"], (int)item["width"], (int)item["height"]),
                    WallpaperPlugin = (string)item["plugin"],
                });
            }
            return result;
        }

        public async Task<List<PlasmaPanel>> ListPanelsAsync()
        {
            const string script = @"
var out = [];
var ps = panels();
for (var i = 0; i < ps.length; i++) {
  var p = ps[i];
  out.push({ id: p.id, screen: p.screen, location: p.location, thickness: p.height, hiding: p.hiding });
}
print(JSON.stringify(out));";
            var json = await EvaluateAsync(script);
            var result = new List<PlasmaPanel>();
            JArray array;
            try
            {
                array = JArray.Parse(json.Trim());
            }
            catch (JsonException ex)
            {
                throw new InvalidOperationException($"plasmashell returned unexpected output for the panel list: {json}", ex);
            }
            foreach (var item in array)
            {
                result.Add(new PlasmaPanel
                {
                    Id = (int)item["id"],
                    Screen = (int)item["screen"],
                    Location = (string)item["location"] ?? string.Empty,
                    Thickness = (int)item["thickness"],
                    Hiding = (string)item["hiding"] ?? "none",
                });
            }
            return result;
        }

        /// <summary>
        /// Switches a desktop to the Lively plugin (if needed) and writes the configuration keys.
        /// </summary>
        public async Task ApplyAsync(int desktopId, IReadOnlyDictionary<string, object> config)
        {
            var writes = string.Join("\n", config.Select(kv => $"  d.writeConfig({JsLiteral(kv.Key)}, {JsLiteral(kv.Value)});"));
            var script = $@"
var ds = desktopsForActivity(currentActivity());
var found = false;
for (var i = 0; i < ds.length; i++) {{
  var d = ds[i];
  if (d.id != {desktopId}) continue;
  found = true;
  if (d.wallpaperPlugin != {JsLiteral(PluginId)}) d.wallpaperPlugin = {JsLiteral(PluginId)};
  d.currentConfigGroup = ['Wallpaper', {JsLiteral(PluginId)}, 'General'];
{writes}
  d.reloadConfig();
}}
print(found ? 'ok' : 'missing');";
            var result = (await EvaluateAsync(script)).Trim();
            if (result != "ok")
                throw new InvalidOperationException($"Plasma desktop {desktopId} no longer exists (plasmashell said '{result}').");
        }

        /// <summary>Puts the desktop back on the plugin it used before Lively took over.</summary>
        public async Task RestorePluginAsync(int desktopId, string previousPlugin)
        {
            var script = $@"
var ds = desktopsForActivity(currentActivity());
for (var i = 0; i < ds.length; i++) {{
  var d = ds[i];
  if (d.id != {desktopId}) continue;
  if (d.wallpaperPlugin == {JsLiteral(PluginId)}) d.wallpaperPlugin = {JsLiteral(previousPlugin)};
  d.reloadConfig();
}}
print('ok');";
            await EvaluateAsync(script);
        }

        public static string JsLiteral(object value)
        {
            switch (value)
            {
                case null: return "''";
                case bool b: return b ? "true" : "false";
                case int i: return i.ToString(CultureInfo.InvariantCulture);
                case long l: return l.ToString(CultureInfo.InvariantCulture);
                case double d: return d.ToString("R", CultureInfo.InvariantCulture);
                case string s: return JsonConvert.ToString(s);
                default: return JsonConvert.ToString(value.ToString());
            }
        }
    }
}

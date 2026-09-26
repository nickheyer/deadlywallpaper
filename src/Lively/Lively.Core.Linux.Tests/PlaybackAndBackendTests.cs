using Lively.Core.Linux.Backends;
using Lively.Core.Linux.Core;
using Lively.Core.Linux.Display;
using Lively.Core.Linux.Plasma;
using Lively.Models;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using Xunit;

namespace Lively.Core.Linux.Tests
{
    public class PlaybackCoverageTests
    {
        private static WaylandToplevel Window(int[] geometry = null, bool fullscreen = false, bool maximized = false, string appId = "org.kde.konsole", string title = "Konsole") =>
            new WaylandToplevel { AppId = appId, Title = title, Geometry = geometry, Fullscreen = fullscreen, Maximized = maximized };

        [Fact]
        public void Fullscreen_and_maximized_always_cover()
        {
            var area = new Rectangle(0, 0, 1920, 1080);
            Assert.True(LinuxPlayback.Covers(Window(fullscreen: true), area));
            // A maximized window leaves the panel strip uncovered but still counts, like IsZoomed on Windows.
            Assert.True(LinuxPlayback.Covers(Window(new[] { 0, 0, 1920, 1036 }, maximized: true), area));
            Assert.True(LinuxPlayback.Covers(Window(maximized: true), new Rectangle(0, 0, 10, 10)));
            Assert.False(LinuxPlayback.Covers(Window(), new Rectangle(0, 0, 10, 10)));
        }

        [Fact]
        public void Window_covers_from_ninety_five_percent_of_the_area()
        {
            var area = new Rectangle(0, 136, 2648, 1490);
            Assert.True(LinuxPlayback.Covers(Window(new[] { 0, 136, 2648, 1490 }), area));
            // 2648 x 1446 of 2648 x 1490 = 97%.
            Assert.True(LinuxPlayback.Covers(Window(new[] { 0, 136, 2648, 1446 }), area));
            // 800 x 600 of the area = 12%.
            Assert.False(LinuxPlayback.Covers(Window(new[] { 100, 200, 800, 600 }), area));
            // 94% is not enough.
            Assert.False(LinuxPlayback.Covers(Window(new[] { 0, 136, 2648, 1400 }), area));
        }

        [Fact]
        public void Grid_pauses_when_the_uncovered_share_reaches_the_threshold()
        {
            var area = new Rectangle(0, 0, 1000, 1000);
            var half = new List<WaylandToplevel> { Window(new[] { 0, 0, 1000, 500 }) };
            // Half of the tiles stay uncovered: far above the 5% default threshold.
            Assert.False(LinuxPlayback.GridCoverage(half, area, 50, 0.05));
            // With a 50% threshold the same window is enough.
            Assert.True(LinuxPlayback.GridCoverage(half, area, 50, 0.5));
            // Two windows that tile the area together leave nothing uncovered.
            var two = new List<WaylandToplevel> { Window(new[] { 0, 0, 1000, 500 }), Window(new[] { 0, 500, 1000, 500 }) };
            Assert.True(LinuxPlayback.GridCoverage(two, area, 50, 0.05));
            // Three windows leaving a 40 px strip: 4% uncovered is within the threshold.
            var strip = new List<WaylandToplevel> { Window(new[] { 0, 0, 500, 960 }), Window(new[] { 500, 0, 500, 960 }) };
            Assert.True(LinuxPlayback.GridCoverage(strip, area, 50, 0.05));
            // A small window never pauses.
            var small = new List<WaylandToplevel> { Window(new[] { 100, 100, 300, 200 }) };
            Assert.False(LinuxPlayback.GridCoverage(small, area, 50, 0.05));
            Assert.False(LinuxPlayback.GridCoverage(new List<WaylandToplevel>(), area, 50, 0.05));
        }

        [Fact]
        public void Grid_counts_partially_overlapped_tiles_and_maximized_windows()
        {
            var area = new Rectangle(0, 0, 1000, 1000);
            // 951 px touch the last tile column and row, so every tile counts as covered.
            var almost = new List<WaylandToplevel> { Window(new[] { 0, 0, 951, 951 }) };
            Assert.True(LinuxPlayback.GridCoverage(almost, area, 50, 0.0));
            var maximized = new List<WaylandToplevel> { Window(new[] { 0, 0, 200, 200 }, maximized: true) };
            Assert.True(LinuxPlayback.GridCoverage(maximized, area, 50, 0.05));
        }

        [Fact]
        public void Visible_window_filter_matches_the_windows_core()
        {
            var visible = Window(new[] { 0, 0, 800, 600 });
            Assert.True(LinuxPlayback.IsVisibleTopLevelWindow(visible, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "firefox", Title = "x", Minimized = true }, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "firefox", Title = "x", SkipTaskbar = true }, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "firefox", Title = "" }, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "firefox", Title = "x", OnCurrentDesktop = false }, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "plasmashell", Title = "Desktop" }, null));
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(new WaylandToplevel { AppId = "lively.ui.avalonia", Title = "Lively Wallpaper" }, null));

            var onOtherActivity = new WaylandToplevel { AppId = "firefox", Title = "x", Activities = new List<string> { "a" } };
            Assert.False(LinuxPlayback.IsVisibleTopLevelWindow(onOtherActivity, "b"));
            Assert.True(LinuxPlayback.IsVisibleTopLevelWindow(onOtherActivity, "a"));
            // No activity manager: activities do not exist, nothing is filtered by them.
            Assert.True(LinuxPlayback.IsVisibleTopLevelWindow(onOtherActivity, null));
            var onAllActivities = new WaylandToplevel { AppId = "firefox", Title = "x" };
            Assert.True(LinuxPlayback.IsVisibleTopLevelWindow(onAllActivities, "b"));
        }

        [Fact]
        public void Shell_and_lively_windows_are_ignored()
        {
            Assert.True(LinuxPlayback.IsShellWindow(new WaylandToplevel { AppId = "plasmashell" }));
            Assert.True(LinuxPlayback.IsLivelyWindow(new WaylandToplevel { AppId = "lively-web-host" }));
            Assert.False(LinuxPlayback.IsShellWindow(new WaylandToplevel { AppId = "firefox" }));
        }
    }

    public class KdeRegistrationTests
    {
        [Fact]
        public void Desktop_file_is_named_after_the_helper_path()
        {
            // sha1("/opt/lively/lively-wl-monitor") starts with these digits; kde-register.sh derives the same name.
            var name = KdeInterfaceRegistration.DesktopFileNameFor("/opt/lively/lively-wl-monitor");
            Assert.StartsWith("lively-wl-monitor-", name);
            Assert.EndsWith(".desktop", name);
            Assert.Equal(8, name.Length - "lively-wl-monitor-".Length - ".desktop".Length);
            Assert.NotEqual(name, KdeInterfaceRegistration.DesktopFileNameFor("/opt/lively/other"));
        }

        [Fact]
        public void Desktop_file_content_declares_both_interfaces()
        {
            var content = KdeInterfaceRegistration.DesktopFileContentFor("/opt/lively/lively-wl-monitor");
            Assert.Contains("Exec=/opt/lively/lively-wl-monitor\n", content);
            Assert.Contains("X-KDE-Wayland-Interfaces=org_kde_plasma_window_management,org_kde_plasma_virtual_desktop_management\n", content);
            Assert.Contains("NoDisplay=true\n", content);
        }
    }

    public class PlasmaBackendTests
    {
        [Fact]
        public void Desktop_is_matched_by_exact_geometry_then_overlap()
        {
            var desktops = new List<PlasmaDesktop>
            {
                new PlasmaDesktop { Id = 1, Screen = 0, Geometry = new Rectangle(0, 136, 2649, 1490) },
                new PlasmaDesktop { Id = 2, Screen = 1, Geometry = new Rectangle(2649, 0, 1080, 1920) },
            };
            var portrait = new DisplayMonitor("DP-3") { DeviceId = "DP-3", Bounds = new Rectangle(2649, 0, 1080, 1920) };
            Assert.Equal(2, PlasmaBackend.FindDesktop(desktops, portrait).Id);

            var slightlyOff = new DisplayMonitor("DP-4") { DeviceId = "DP-4", Bounds = new Rectangle(0, 130, 2649, 1490) };
            Assert.Equal(1, PlasmaBackend.FindDesktop(desktops, slightlyOff).Id);

            var nowhere = new DisplayMonitor("X") { DeviceId = "X", Bounds = new Rectangle(9000, 9000, 10, 10) };
            Assert.Null(PlasmaBackend.FindDesktop(desktops, nowhere));
        }

        [Fact]
        public void Work_area_subtracts_only_panels_that_reserve_space()
        {
            var bounds = new Rectangle(0, 136, 2648, 1490);
            var panels = new List<PlasmaPanel>
            {
                new PlasmaPanel { Id = 1, Screen = 0, Location = "bottom", Thickness = 44, Hiding = "none" },
                new PlasmaPanel { Id = 2, Screen = 0, Location = "top", Thickness = 30, Hiding = "autohide" },
                new PlasmaPanel { Id = 3, Screen = 0, Location = "left", Thickness = 60, Hiding = "none" },
                new PlasmaPanel { Id = 4, Screen = 1, Location = "top", Thickness = 100, Hiding = "none" },
            };
            var area = PlasmaBackend.SubtractPanels(bounds, panels.FindAll(p => p.Screen == 0));
            Assert.Equal(new Rectangle(60, 136, 2588, 1446), area);

            var desktops = new List<PlasmaDesktop>
            {
                new PlasmaDesktop { Id = 1, Screen = 0, Geometry = bounds },
                new PlasmaDesktop { Id = 2, Screen = 1, Geometry = new Rectangle(2649, 0, 1080, 1920) },
            };
            var displays = new List<DisplayMonitor>
            {
                new DisplayMonitor("DP-4") { DeviceId = "DP-4", Bounds = bounds },
                new DisplayMonitor("DP-3") { DeviceId = "DP-3", Bounds = new Rectangle(2649, 0, 1080, 1920) },
                new DisplayMonitor("X") { DeviceId = "X", Bounds = new Rectangle(9000, 9000, 10, 10) },
            };
            var areas = PlasmaBackend.ComputeWorkAreas(displays, desktops, panels);
            Assert.Equal(new Rectangle(60, 136, 2588, 1446), areas["DP-4"]);
            Assert.Equal(new Rectangle(2649, 100, 1080, 1820), areas["DP-3"]);
            Assert.Equal(new Rectangle(9000, 9000, 10, 10), areas["X"]);
        }

        [Fact]
        public void Registry_persists_previous_plugins()
        {
            var path = Path.Combine(Path.GetTempPath(), "lively-test-" + Path.GetRandomFileName(), "plasma.json");
            var registry = new PlasmaDesktopRegistry(path);
            registry.Remember(5, "org.kde.image");
            registry.Remember(5, PlasmaShellScripting.PluginId); // must not overwrite the real previous plugin
            Assert.Equal("org.kde.image", registry.PreviousPluginFor(5, PlasmaShellScripting.PluginId));

            var reloaded = new PlasmaDesktopRegistry(path);
            Assert.Equal("org.kde.image", reloaded.PreviousPluginFor(5, "whatever"));
            reloaded.Forget(5);
            Assert.False(File.Exists(path));
            Directory.Delete(Path.GetDirectoryName(path), true);
        }

        [Fact]
        public void Package_hash_changes_with_content()
        {
            var dir = Path.Combine(Path.GetTempPath(), "lively-test-" + Path.GetRandomFileName());
            Directory.CreateDirectory(Path.Combine(dir, "contents"));
            File.WriteAllText(Path.Combine(dir, "metadata.json"), "{}");
            File.WriteAllText(Path.Combine(dir, "contents", "main.qml"), "Item {}");
            var first = PlasmaPluginInstaller.HashDirectory(dir);
            File.WriteAllText(Path.Combine(dir, "contents", "main.qml"), "Item { }");
            var second = PlasmaPluginInstaller.HashDirectory(dir);
            Assert.NotEqual(first, second);
            Directory.Delete(dir, true);
        }
    }

    public class HostFactoryTests
    {
        [Fact]
        public void Program_wallpapers_are_rejected_on_wayland()
        {
            Assert.True(HostWallpaperFactory.IsProgramWallpaper(Models.Enums.WallpaperType.unity));
            Assert.True(HostWallpaperFactory.IsProgramWallpaper(Models.Enums.WallpaperType.app));
            Assert.False(HostWallpaperFactory.IsProgramWallpaper(Models.Enums.WallpaperType.video));
            Assert.False(HostWallpaperFactory.IsProgramWallpaper(Models.Enums.WallpaperType.web));
        }
    }
}

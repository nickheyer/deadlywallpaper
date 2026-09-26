using Lively.Models.Services;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// One entry of the tray menu: a clickable item or a separator.
    /// </summary>
    public sealed class MenuEntry
    {
        internal MenuEntry(int id, TrayMenuItem? item, bool isToggle)
        {
            Id = id;
            Item = item;
            IsToggle = isToggle;
            Label = string.Empty;
            Enabled = true;
        }

        public int Id { get; }
        public TrayMenuItem? Item { get; }
        public bool IsSeparator => Item is null;
        public bool IsToggle { get; }
        public string Label { get; internal set; }
        public bool Enabled { get; internal set; }
        public bool Checked { get; internal set; }
    }

    /// <summary>
    /// The tray menu as a com.canonical.dbusmenu layout, independent of any bus connection: item ids, order,
    /// separators, localized labels, enabled and checkmark state, and the mapping from click to command.
    /// Order mirrors the Windows tray: open app | close wallpapers, pause, change wallpaper, customise |
    /// update (optional) | report bug | exit.
    /// </summary>
    public sealed class MenuModel
    {
        public const int RootId = 0;
        public const string SeparatorType = "separator";
        public const string StandardType = "standard";
        public const string CheckmarkToggleType = "checkmark";
        public const string SubmenuChildrenDisplay = "submenu";

        private readonly TrayCommands commands;
        private readonly List<MenuEntry> entries = [];
        private readonly Dictionary<int, MenuEntry> entriesById = [];
        private readonly object sync = new();

        public MenuModel(TrayCommands commands)
        {
            this.commands = commands ?? throw new ArgumentNullException(nameof(commands));
            Require(commands.OpenApp, nameof(TrayCommands.OpenApp));
            Require(commands.CloseWallpapers, nameof(TrayCommands.CloseWallpapers));
            Require(commands.TogglePause, nameof(TrayCommands.TogglePause));
            Require(commands.ChangeWallpaper, nameof(TrayCommands.ChangeWallpaper));
            Require(commands.CustomiseWallpaper, nameof(TrayCommands.CustomiseWallpaper));
            Require(commands.ShowUpdatePage, nameof(TrayCommands.ShowUpdatePage));
            Require(commands.ReportBug, nameof(TrayCommands.ReportBug));
            Require(commands.Exit, nameof(TrayCommands.Exit));
            Require(commands.IsPaused, nameof(TrayCommands.IsPaused));
            Require(commands.CanCustomise, nameof(TrayCommands.CanCustomise));
            Require(commands.UpdateStatus, nameof(TrayCommands.UpdateStatus));
            Require(commands.GetString, nameof(TrayCommands.GetString));

            var id = RootId + 1;
            Add(new MenuEntry(id++, TrayMenuItem.openApp, isToggle: false));
            Add(new MenuEntry(id++, null, isToggle: false));
            Add(new MenuEntry(id++, TrayMenuItem.closeWallpaper, isToggle: false));
            Add(new MenuEntry(id++, TrayMenuItem.pauseWallpaper, isToggle: true));
            Add(new MenuEntry(id++, TrayMenuItem.changeWallpaper, isToggle: false));
            Add(new MenuEntry(id++, TrayMenuItem.customiseWallpaper, isToggle: false));
            if (commands.ShowUpdateItem)
            {
                Add(new MenuEntry(id++, null, isToggle: false));
                Add(new MenuEntry(id++, TrayMenuItem.updateApp, isToggle: false));
            }
            Add(new MenuEntry(id++, null, isToggle: false));
            Add(new MenuEntry(id++, TrayMenuItem.reportBug, isToggle: false));
            Add(new MenuEntry(id++, null, isToggle: false));
            Add(new MenuEntry(id++, TrayMenuItem.exitApp, isToggle: false));

            Refresh();
            Revision = 1;
        }

        /// <summary>Layout revision; increases whenever <see cref="Refresh"/> changes any item.</summary>
        public uint Revision { get; private set; }

        /// <summary>All entries below the root, in menu order.</summary>
        public IReadOnlyList<MenuEntry> Entries => entries;

        public bool TryGetEntry(int id, out MenuEntry entry) => entriesById.TryGetValue(id, out entry);

        /// <exception cref="KeyNotFoundException">No entry has <paramref name="id"/>.</exception>
        public MenuEntry GetEntry(int id)
        {
            return entriesById.TryGetValue(id, out var entry)
                ? entry
                : throw new KeyNotFoundException($"The tray menu has no item with id {id}.");
        }

        /// <summary>
        /// GetLayout: the node for <paramref name="parentId"/> with its children (none when <paramref name="recursionDepth"/> is 0).
        /// </summary>
        public (int id, IDictionary<string, object> properties, object[] children) BuildLayout(int parentId, int recursionDepth, string[] propertyNames)
        {
            lock (sync)
            {
                if (parentId == RootId)
                {
                    var children = recursionDepth == 0
                        ? []
                        : entries.Select(entry => Node(entry.Id, ItemProperties(entry, propertyNames))).ToArray();
                    return (RootId, RootProperties(propertyNames), children);
                }

                var single = GetEntry(parentId);
                return (single.Id, ItemProperties(single, propertyNames), []);
            }
        }

        /// <summary>
        /// GetGroupProperties: properties of the requested ids (all nodes including the root when <paramref name="ids"/> is empty);
        /// unknown ids are left out.
        /// </summary>
        public (int, IDictionary<string, object>)[] GetGroupProperties(int[] ids, string[] propertyNames)
        {
            lock (sync)
            {
                var targets = ids is null || ids.Length == 0
                    ? new[] { RootId }.Concat(entries.Select(entry => entry.Id))
                    : ids.Where(id => id == RootId || entriesById.ContainsKey(id));
                return targets
                    .Select(id => (id, id == RootId ? RootProperties(propertyNames) : ItemProperties(GetEntry(id), propertyNames)))
                    .ToArray();
            }
        }

        /// <summary>
        /// GetProperty: the value of one property including the dbusmenu defaults for properties the layout leaves implicit.
        /// </summary>
        /// <exception cref="KeyNotFoundException">Unknown id or property name.</exception>
        public object GetProperty(int id, string name)
        {
            lock (sync)
            {
                var all = id == RootId ? RootPropertiesWithDefaults() : ItemPropertiesWithDefaults(GetEntry(id));
                return all.TryGetValue(name, out var value)
                    ? value
                    : throw new KeyNotFoundException($"Tray menu item {id} has no property '{name}'.");
            }
        }

        /// <summary>
        /// Runs the command behind a clicked item. Returns false for separators and disabled items.
        /// </summary>
        /// <exception cref="KeyNotFoundException">No entry has <paramref name="id"/>.</exception>
        public bool Invoke(int id)
        {
            var entry = GetEntry(id);
            if (entry.IsSeparator)
                return false;
            lock (sync)
            {
                if (!entry.Enabled)
                    return false;
            }
            CommandFor(entry.Item.Value)();
            return true;
        }

        /// <summary>
        /// Re-reads labels, enabled state and checkmarks from <see cref="TrayCommands"/> and returns the changed
        /// properties per item; bumps <see cref="Revision"/> when anything changed.
        /// </summary>
        public IReadOnlyList<(int id, IDictionary<string, object> changed)> Refresh()
        {
            var states = entries
                .Where(entry => !entry.IsSeparator)
                .Select(entry => (entry, state: ComputeState(entry.Item.Value)))
                .ToList();

            lock (sync)
            {
                var changes = new List<(int, IDictionary<string, object>)>();
                foreach (var (entry, state) in states)
                {
                    var changed = new Dictionary<string, object>();
                    if (!string.Equals(state.label, entry.Label, StringComparison.Ordinal))
                    {
                        entry.Label = state.label;
                        changed["label"] = state.label;
                    }
                    if (state.enabled != entry.Enabled)
                    {
                        entry.Enabled = state.enabled;
                        changed["enabled"] = state.enabled;
                    }
                    if (entry.IsToggle && state.isChecked != entry.Checked)
                    {
                        entry.Checked = state.isChecked;
                        changed["toggle-state"] = ToggleState(entry);
                    }
                    if (changed.Count > 0)
                        changes.Add((entry.Id, changed));
                }
                if (changes.Count > 0)
                    Revision++;
                return changes;
            }
        }

        private void Add(MenuEntry entry)
        {
            entries.Add(entry);
            entriesById.Add(entry.Id, entry);
        }

        private Action CommandFor(TrayMenuItem item) => item switch
        {
            TrayMenuItem.openApp => commands.OpenApp,
            TrayMenuItem.closeWallpaper => commands.CloseWallpapers,
            TrayMenuItem.pauseWallpaper => commands.TogglePause,
            TrayMenuItem.changeWallpaper => commands.ChangeWallpaper,
            TrayMenuItem.customiseWallpaper => commands.CustomiseWallpaper,
            TrayMenuItem.updateApp => commands.ShowUpdatePage,
            TrayMenuItem.reportBug => commands.ReportBug,
            TrayMenuItem.exitApp => commands.Exit,
            _ => throw new ArgumentOutOfRangeException(nameof(item), item, "Unknown tray menu item."),
        };

        private (string label, bool enabled, bool isChecked) ComputeState(TrayMenuItem item) => item switch
        {
            TrayMenuItem.openApp => (Localized("TextOpenLively"), true, false),
            TrayMenuItem.closeWallpaper => (Localized("TextCloseWallpapers"), true, false),
            TrayMenuItem.pauseWallpaper => (Localized("TextPauseWallpapers"), true, commands.IsPaused()),
            TrayMenuItem.changeWallpaper => (Localized("TextChangeWallpaper"), true, false),
            TrayMenuItem.customiseWallpaper => (Localized("TextCustomiseWallpaper"), commands.CanCustomise(), false),
            TrayMenuItem.updateApp => UpdateState(commands.UpdateStatus()),
            TrayMenuItem.reportBug => (Localized("ReportBug/Header"), true, false),
            TrayMenuItem.exitApp => (Localized("TextExit"), true, false),
            _ => throw new ArgumentOutOfRangeException(nameof(item), item, "Unknown tray menu item."),
        };

        private (string label, bool enabled, bool isChecked) UpdateState(AppUpdateStatus status) => status switch
        {
            AppUpdateStatus.uptodate => (Localized("TextUpdateUptodate"), false, false),
            AppUpdateStatus.available => (Localized("TextUpdateAvailable"), true, false),
            AppUpdateStatus.invalid => ("Fancy~", false, false),
            AppUpdateStatus.notchecked => (Localized("TextUpdateChecking"), false, false),
            AppUpdateStatus.error => (Localized("TextupdateCheckFail"), true, false),
            _ => throw new ArgumentOutOfRangeException(nameof(status), status, "Unknown update status."),
        };

        private string Localized(string key)
        {
            return commands.GetString(key)
                ?? throw new InvalidOperationException($"TrayCommands.GetString returned null for resource key '{key}'.");
        }

        private static object Node(int id, IDictionary<string, object> properties)
        {
            (int, IDictionary<string, object>, object[]) node = (id, properties, []);
            return node;
        }

        private static IDictionary<string, object> RootProperties(string[] propertyNames)
        {
            return Filter(new Dictionary<string, object> { ["children-display"] = SubmenuChildrenDisplay }, propertyNames);
        }

        private static IDictionary<string, object> ItemProperties(MenuEntry entry, string[] propertyNames)
        {
            var properties = new Dictionary<string, object>();
            if (entry.IsSeparator)
            {
                properties["type"] = SeparatorType;
            }
            else
            {
                properties["label"] = entry.Label;
                properties["enabled"] = entry.Enabled;
                if (entry.IsToggle)
                {
                    properties["toggle-type"] = CheckmarkToggleType;
                    properties["toggle-state"] = ToggleState(entry);
                }
            }
            return Filter(properties, propertyNames);
        }

        private static Dictionary<string, object> RootPropertiesWithDefaults() => new()
        {
            ["type"] = StandardType,
            ["label"] = string.Empty,
            ["enabled"] = true,
            ["visible"] = true,
            ["toggle-type"] = string.Empty,
            ["toggle-state"] = -1,
            ["children-display"] = SubmenuChildrenDisplay,
        };

        private static Dictionary<string, object> ItemPropertiesWithDefaults(MenuEntry entry) => new()
        {
            ["type"] = entry.IsSeparator ? SeparatorType : StandardType,
            ["label"] = entry.Label,
            ["enabled"] = entry.Enabled,
            ["visible"] = true,
            ["toggle-type"] = entry.IsToggle ? CheckmarkToggleType : string.Empty,
            ["toggle-state"] = entry.IsToggle ? ToggleState(entry) : -1,
            ["children-display"] = string.Empty,
        };

        private static int ToggleState(MenuEntry entry) => entry.Checked ? 1 : 0;

        private static IDictionary<string, object> Filter(Dictionary<string, object> properties, string[] propertyNames)
        {
            if (propertyNames is null || propertyNames.Length == 0)
                return properties;
            var wanted = new HashSet<string>(propertyNames, StringComparer.Ordinal);
            return properties.Where(pair => wanted.Contains(pair.Key)).ToDictionary(pair => pair.Key, pair => pair.Value);
        }

        private static void Require(Delegate value, string name)
        {
            if (value is null)
                throw new ArgumentException($"TrayCommands.{name} must be set.", nameof(commands));
        }
    }
}

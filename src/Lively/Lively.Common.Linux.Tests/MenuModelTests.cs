using Lively.Common.Linux.DBus.StatusNotifier;
using Lively.Models.Services;
using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace Lively.Common.Linux.Tests
{
    public sealed class MenuModelTests
    {
        private readonly List<string> invoked = [];
        private bool paused;
        private bool canCustomise;
        private AppUpdateStatus updateStatus = AppUpdateStatus.notchecked;

        private TrayCommands CreateCommands(bool showUpdateItem = true) => new()
        {
            OpenApp = () => invoked.Add("OpenApp"),
            CloseWallpapers = () => invoked.Add("CloseWallpapers"),
            TogglePause = () => { paused = !paused; invoked.Add("TogglePause"); },
            ChangeWallpaper = () => invoked.Add("ChangeWallpaper"),
            CustomiseWallpaper = () => invoked.Add("CustomiseWallpaper"),
            ShowUpdatePage = () => invoked.Add("ShowUpdatePage"),
            ReportBug = () => invoked.Add("ReportBug"),
            Exit = () => invoked.Add("Exit"),
            IsPaused = () => paused,
            CanCustomise = () => canCustomise,
            UpdateStatus = () => updateStatus,
            ShowUpdateItem = showUpdateItem,
            GetString = key => "L:" + key,
        };

        private static MenuEntry Entry(MenuModel model, TrayMenuItem item) => model.Entries.Single(entry => entry.Item == item);

        [Fact]
        public void EntriesFollowTheWindowsTrayOrderWithSeparators()
        {
            var model = new MenuModel(CreateCommands());

            TrayMenuItem?[] expected =
            [
                TrayMenuItem.openApp,
                null,
                TrayMenuItem.closeWallpaper,
                TrayMenuItem.pauseWallpaper,
                TrayMenuItem.changeWallpaper,
                TrayMenuItem.customiseWallpaper,
                null,
                TrayMenuItem.updateApp,
                null,
                TrayMenuItem.reportBug,
                null,
                TrayMenuItem.exitApp,
            ];
            Assert.Equal(expected, model.Entries.Select(entry => entry.Item).ToArray());
            Assert.Equal(Enumerable.Range(1, expected.Length), model.Entries.Select(entry => entry.Id));
        }

        [Fact]
        public void UpdateItemAndItsSeparatorAreOmittedWhenHidden()
        {
            var model = new MenuModel(CreateCommands(showUpdateItem: false));

            TrayMenuItem?[] expected =
            [
                TrayMenuItem.openApp,
                null,
                TrayMenuItem.closeWallpaper,
                TrayMenuItem.pauseWallpaper,
                TrayMenuItem.changeWallpaper,
                TrayMenuItem.customiseWallpaper,
                null,
                TrayMenuItem.reportBug,
                null,
                TrayMenuItem.exitApp,
            ];
            Assert.Equal(expected, model.Entries.Select(entry => entry.Item).ToArray());
        }

        [Fact]
        public void LabelsUseTheWindowsResourceKeys()
        {
            var model = new MenuModel(CreateCommands());

            Assert.Equal("L:TextOpenLively", Entry(model, TrayMenuItem.openApp).Label);
            Assert.Equal("L:TextCloseWallpapers", Entry(model, TrayMenuItem.closeWallpaper).Label);
            Assert.Equal("L:TextPauseWallpapers", Entry(model, TrayMenuItem.pauseWallpaper).Label);
            Assert.Equal("L:TextChangeWallpaper", Entry(model, TrayMenuItem.changeWallpaper).Label);
            Assert.Equal("L:TextCustomiseWallpaper", Entry(model, TrayMenuItem.customiseWallpaper).Label);
            Assert.Equal("L:TextUpdateChecking", Entry(model, TrayMenuItem.updateApp).Label);
            Assert.Equal("L:ReportBug/Header", Entry(model, TrayMenuItem.reportBug).Label);
            Assert.Equal("L:TextExit", Entry(model, TrayMenuItem.exitApp).Label);
        }

        [Fact]
        public void RootLayoutListsEveryEntryWithItsProperties()
        {
            var model = new MenuModel(CreateCommands());

            var (rootId, rootProperties, children) = model.BuildLayout(MenuModel.RootId, -1, []);

            Assert.Equal(MenuModel.RootId, rootId);
            Assert.Equal("submenu", rootProperties["children-display"]);
            Assert.Equal(model.Entries.Count, children.Length);
            var nodes = children.Cast<(int, IDictionary<string, object>, object[])>().ToArray();
            Assert.Equal(model.Entries.Select(entry => entry.Id), nodes.Select(node => node.Item1));
            Assert.All(nodes, node => Assert.Empty(node.Item3));

            var openApp = nodes.Single(node => node.Item1 == Entry(model, TrayMenuItem.openApp).Id).Item2;
            Assert.Equal("L:TextOpenLively", openApp["label"]);
            Assert.Equal(true, openApp["enabled"]);
            Assert.False(openApp.ContainsKey("toggle-type"));

            var separator = nodes.Single(node => node.Item1 == 2).Item2;
            Assert.Equal("separator", separator["type"]);
            Assert.False(separator.ContainsKey("label"));
        }

        [Fact]
        public void RootLayoutWithZeroDepthHasNoChildren()
        {
            var model = new MenuModel(CreateCommands());

            var (_, _, children) = model.BuildLayout(MenuModel.RootId, 0, []);

            Assert.Empty(children);
        }

        [Fact]
        public void PropertyNamesFilterTheLayout()
        {
            var model = new MenuModel(CreateCommands());

            var (_, rootProperties, children) = model.BuildLayout(MenuModel.RootId, -1, ["label"]);

            Assert.Empty(rootProperties);
            foreach (var (id, properties, _) in children.Cast<(int, IDictionary<string, object>, object[])>())
            {
                var entry = model.GetEntry(id);
                if (entry.IsSeparator)
                    Assert.Empty(properties);
                else
                    Assert.Equal(["label"], properties.Keys);
            }
        }

        [Fact]
        public void PauseItemIsACheckmarkToggleThatReflectsIsPaused()
        {
            paused = true;
            var model = new MenuModel(CreateCommands());
            var pause = Entry(model, TrayMenuItem.pauseWallpaper);

            Assert.True(pause.IsToggle);
            Assert.True(pause.Checked);
            var (_, properties, _) = model.BuildLayout(pause.Id, -1, []);
            Assert.Equal("checkmark", properties["toggle-type"]);
            Assert.Equal(1, properties["toggle-state"]);

            paused = false;
            var changes = model.Refresh();

            var change = Assert.Single(changes);
            Assert.Equal(pause.Id, change.id);
            Assert.Equal(0, change.changed["toggle-state"]);
            Assert.False(pause.Checked);
        }

        [Fact]
        public void ClickingPauseTogglesAndUpdatesTheCheckmark()
        {
            var model = new MenuModel(CreateCommands());
            var pause = Entry(model, TrayMenuItem.pauseWallpaper);

            Assert.True(model.Invoke(pause.Id));
            var changes = model.Refresh();

            Assert.Equal(["TogglePause"], invoked);
            Assert.True(pause.Checked);
            Assert.Equal(1, Assert.Single(changes).changed["toggle-state"]);
        }

        [Fact]
        public void CustomiseIsEnabledOnlyWhenACustomisableWallpaperIsRunning()
        {
            var model = new MenuModel(CreateCommands());
            var customise = Entry(model, TrayMenuItem.customiseWallpaper);

            Assert.False(customise.Enabled);
            Assert.False(model.Invoke(customise.Id));
            Assert.Empty(invoked);

            canCustomise = true;
            var changes = model.Refresh();

            Assert.Equal(true, Assert.Single(changes).changed["enabled"]);
            Assert.True(customise.Enabled);
            Assert.True(model.Invoke(customise.Id));
            Assert.Equal(["CustomiseWallpaper"], invoked);
        }

        [Theory]
        [InlineData(AppUpdateStatus.uptodate, "L:TextUpdateUptodate", false)]
        [InlineData(AppUpdateStatus.available, "L:TextUpdateAvailable", true)]
        [InlineData(AppUpdateStatus.invalid, "Fancy~", false)]
        [InlineData(AppUpdateStatus.notchecked, "L:TextUpdateChecking", false)]
        [InlineData(AppUpdateStatus.error, "L:TextupdateCheckFail", true)]
        public void UpdateItemReflectsTheUpdateStatus(AppUpdateStatus status, string label, bool enabled)
        {
            updateStatus = status;
            var model = new MenuModel(CreateCommands());
            var update = Entry(model, TrayMenuItem.updateApp);

            Assert.Equal(label, update.Label);
            Assert.Equal(enabled, update.Enabled);
        }

        [Fact]
        public void UpdateStatusChangeIsReportedAsLabelAndEnabledChange()
        {
            var model = new MenuModel(CreateCommands());
            var update = Entry(model, TrayMenuItem.updateApp);

            updateStatus = AppUpdateStatus.available;
            var changes = model.Refresh();

            var change = Assert.Single(changes);
            Assert.Equal(update.Id, change.id);
            Assert.Equal("L:TextUpdateAvailable", change.changed["label"]);
            Assert.Equal(true, change.changed["enabled"]);
            Assert.True(model.Invoke(update.Id));
            Assert.Equal(["ShowUpdatePage"], invoked);
        }

        [Fact]
        public void EveryItemClickRunsItsCommand()
        {
            canCustomise = true;
            updateStatus = AppUpdateStatus.available;
            var model = new MenuModel(CreateCommands());

            foreach (var entry in model.Entries.Where(entry => !entry.IsSeparator))
                Assert.True(model.Invoke(entry.Id));

            Assert.Equal(
                ["OpenApp", "CloseWallpapers", "TogglePause", "ChangeWallpaper", "CustomiseWallpaper", "ShowUpdatePage", "ReportBug", "Exit"],
                invoked);
        }

        [Fact]
        public void SeparatorsAndUnknownIdsCannotBeClicked()
        {
            var model = new MenuModel(CreateCommands());

            Assert.False(model.Invoke(2));
            Assert.Throws<KeyNotFoundException>(() => model.Invoke(999));
            Assert.Empty(invoked);
        }

        [Fact]
        public void RefreshBumpsTheRevisionOnlyWhenSomethingChanged()
        {
            var model = new MenuModel(CreateCommands());
            var revision = model.Revision;

            Assert.Empty(model.Refresh());
            Assert.Equal(revision, model.Revision);

            paused = true;
            Assert.NotEmpty(model.Refresh());
            Assert.Equal(revision + 1, model.Revision);
        }

        [Fact]
        public void GroupPropertiesWithoutIdsReturnsRootAndEveryEntry()
        {
            var model = new MenuModel(CreateCommands());

            var groups = model.GetGroupProperties([], []);

            Assert.Equal(new[] { MenuModel.RootId }.Concat(model.Entries.Select(entry => entry.Id)), groups.Select(group => group.Item1));
            Assert.Equal("submenu", groups[0].Item2["children-display"]);
        }

        [Fact]
        public void GroupPropertiesSkipsUnknownIds()
        {
            var model = new MenuModel(CreateCommands());

            var groups = model.GetGroupProperties([1, 999, 3], ["label"]);

            Assert.Equal([1, 3], groups.Select(group => group.Item1));
            Assert.Equal("L:TextOpenLively", groups[0].Item2["label"]);
        }

        [Fact]
        public void GetPropertyIncludesDbusmenuDefaults()
        {
            var model = new MenuModel(CreateCommands());
            var openApp = Entry(model, TrayMenuItem.openApp);

            Assert.Equal("standard", model.GetProperty(openApp.Id, "type"));
            Assert.Equal(true, model.GetProperty(openApp.Id, "visible"));
            Assert.Equal(-1, model.GetProperty(openApp.Id, "toggle-state"));
            Assert.Equal("separator", model.GetProperty(2, "type"));
            Assert.Equal("submenu", model.GetProperty(MenuModel.RootId, "children-display"));
            Assert.Throws<KeyNotFoundException>(() => model.GetProperty(openApp.Id, "no-such-property"));
        }

        [Fact]
        public void MissingCommandsAreRejected()
        {
            var commands = CreateCommands();
            commands.TogglePause = null;

            var error = Assert.Throws<ArgumentException>(() => new MenuModel(commands));
            Assert.Contains("TogglePause", error.Message);
        }

        [Fact]
        public void NullLocalizedStringIsRejected()
        {
            var commands = CreateCommands();
            commands.GetString = _ => null;

            Assert.Throws<InvalidOperationException>(() => new MenuModel(commands));
        }
    }
}

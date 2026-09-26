using Lively.Common.Linux.Power;
using System;
using System.IO;
using Xunit;

namespace Lively.Common.Linux.Tests
{
    public sealed class SysfsPowerSupplyTests : IDisposable
    {
        private readonly string root;

        public SysfsPowerSupplyTests()
        {
            root = Path.Combine(Path.GetTempPath(), "lively-power-tests", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
        }

        public void Dispose()
        {
            Directory.Delete(root, recursive: true);
        }

        private void AddSupply(string name, string type, string online = null)
        {
            var dir = Path.Combine(root, name);
            Directory.CreateDirectory(dir);
            File.WriteAllText(Path.Combine(dir, "type"), type + "\n");
            if (online != null)
                File.WriteAllText(Path.Combine(dir, "online"), online + "\n");
        }

        [Fact]
        public void MainsOnlineMeansNotOnBattery()
        {
            AddSupply("AC", "Mains", "1");
            AddSupply("BAT0", "Battery");

            Assert.False(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void MainsOfflineMeansOnBattery()
        {
            AddSupply("AC", "Mains", "0");
            AddSupply("BAT0", "Battery");

            Assert.True(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void AnyOnlineMainsSupplyMeansNotOnBattery()
        {
            AddSupply("AC0", "Mains", "0");
            AddSupply("ucsi-source-psy-USBC000:001", "Mains", "1");

            Assert.False(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void WithoutAnyMainsSupplyReportsNotOnBattery()
        {
            AddSupply("BAT0", "Battery");

            Assert.False(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void EmptyRootReportsNotOnBattery()
        {
            Assert.False(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void MissingRootReportsNotOnBattery()
        {
            Assert.False(new SysfsPowerSupply(Path.Combine(root, "missing")).IsOnBattery());
        }

        [Fact]
        public void MainsWithoutOnlineAttributeIsNotCounted()
        {
            AddSupply("AC", "Mains");

            Assert.False(new SysfsPowerSupply(root).IsOnBattery());
        }

        [Fact]
        public void SupplyEntriesMayBeSymlinks()
        {
            var target = Path.Combine(root, "..", "lively-power-target-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(target);
            try
            {
                File.WriteAllText(Path.Combine(target, "type"), "Mains\n");
                File.WriteAllText(Path.Combine(target, "online"), "0\n");
                Directory.CreateSymbolicLink(Path.Combine(root, "AC"), target);

                Assert.True(new SysfsPowerSupply(root).IsOnBattery());
            }
            finally
            {
                Directory.Delete(target, recursive: true);
            }
        }

        [Fact]
        public void PowerStateServiceRefreshTracksTheSupplyAndRaisesTheEvent()
        {
            AddSupply("AC", "Mains", "1");
            using var service = new PowerStateService(TimeSpan.FromSeconds(30), root);
            var raised = new System.Collections.Generic.List<bool>();
            service.PowerSourceChanged += (_, onBattery) => raised.Add(onBattery);

            service.RefreshPowerSupply();
            Assert.False(service.IsOnBattery);
            Assert.Empty(raised);

            File.WriteAllText(Path.Combine(root, "AC", "online"), "0\n");
            service.RefreshPowerSupply();
            Assert.True(service.IsOnBattery);
            Assert.Equal([true], raised);

            service.RefreshPowerSupply();
            Assert.Equal([true], raised);
        }

        [Fact]
        public void NonPositiveRefreshIntervalIsRejected()
        {
            Assert.Throws<ArgumentOutOfRangeException>(() => new PowerStateService(TimeSpan.Zero, root));
        }
    }
}

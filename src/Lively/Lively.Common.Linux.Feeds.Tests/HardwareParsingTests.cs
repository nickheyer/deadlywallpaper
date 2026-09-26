using Lively.Common.Linux.Hardware;
using System;
using Xunit;

namespace Lively.Common.Linux.Feeds.Tests
{
    public class HardwareParsingTests
    {
        private const string ProcStatBefore =
            "cpu  1000 100 500 8000 200 50 30 20 0 0\n" +
            "cpu0 500 50 250 4000 100 25 15 10 0 0\n" +
            "intr 12345\n";

        // +1000 busy jiffies (user 600, system 300, irq 50, softirq 50) and +1000 idle jiffies (idle 800, iowait 200) → 50 %.
        private const string ProcStatAfter =
            "cpu  1600 100 800 8800 400 100 80 20 0 0\n" +
            "cpu0 800 50 400 4400 200 50 40 10 0 0\n" +
            "intr 12345\n";

        [Fact]
        public void CpuPercentFromTwoProcStatSamples()
        {
            var before = LinuxHardwareUsageService.ParseProcStatCpu(ProcStatBefore);
            var after = LinuxHardwareUsageService.ParseProcStatCpu(ProcStatAfter);

            Assert.Equal(9900, before.Total);
            Assert.Equal(8200, before.Idle);
            Assert.Equal(50f, LinuxHardwareUsageService.CpuUsagePercent(before, after), 3);
        }

        [Fact]
        public void CpuPercentIsZeroWithoutElapsedJiffies()
        {
            var sample = LinuxHardwareUsageService.ParseProcStatCpu(ProcStatBefore);
            Assert.Equal(0f, LinuxHardwareUsageService.CpuUsagePercent(sample, sample));
        }

        [Fact]
        public void CpuPercentIsFullWhenNoIdleTimePassed()
        {
            var before = new LinuxHardwareUsageService.CpuSample(Idle: 100, Total: 1000);
            var after = new LinuxHardwareUsageService.CpuSample(Idle: 100, Total: 1500);
            Assert.Equal(100f, LinuxHardwareUsageService.CpuUsagePercent(before, after));
        }

        [Fact]
        public void ProcStatWithoutCpuLineThrows()
        {
            Assert.Throws<FormatException>(() => LinuxHardwareUsageService.ParseProcStatCpu("intr 1 2 3\nctxt 4\n"));
        }

        [Fact]
        public void MemInfoParsesTotalAndAvailableInMegabytes()
        {
            const string memInfo =
                "MemTotal:       65618524 kB\n" +
                "MemFree:         4684408 kB\n" +
                "MemAvailable:   46493388 kB\n" +
                "Buffers:          123456 kB\n";

            var (totalMb, availableMb) = LinuxHardwareUsageService.ParseMemInfo(memInfo);

            Assert.Equal(65618524 / 1024, totalMb);
            Assert.Equal(46493388 / 1024f, availableMb, 3);
        }

        [Fact]
        public void MemInfoWithoutMemAvailableThrows()
        {
            Assert.Throws<FormatException>(() => LinuxHardwareUsageService.ParseMemInfo("MemTotal:       65618524 kB\nMemFree:         4684408 kB\n"));
        }

        [Fact]
        public void DefaultRouteInterfaceIsTheGatewayRouteWithLowestMetric()
        {
            const string route =
                "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n" +
                "wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n" +
                "enp8s0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n" +
                "docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n" +
                "tun0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\t0\t0\t0\n";

            Assert.Equal("enp8s0", LinuxHardwareUsageService.ParseDefaultRouteInterface(route));
        }

        [Fact]
        public void NoDefaultRouteGivesNull()
        {
            const string route =
                "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n" +
                "docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";

            Assert.Null(LinuxHardwareUsageService.ParseDefaultRouteInterface(route));
        }

        [Fact]
        public void NetDevParsesReceiveAndTransmitBytesOfTheNamedInterface()
        {
            const string netDev =
                "Inter-|   Receive                                                |  Transmit\n" +
                " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n" +
                "    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n" +
                "enp8s0: 123456789 100 0 0 0 0 0 5 987654321 200 0 0 0 0 0 0\n";

            var (rx, tx) = LinuxHardwareUsageService.ParseNetDev(netDev, "enp8s0");

            Assert.Equal(123456789, rx);
            Assert.Equal(987654321, tx);
            Assert.Throws<FormatException>(() => LinuxHardwareUsageService.ParseNetDev(netDev, "wlan0"));
        }

        [Fact]
        public void CpuModelNameComesFromCpuInfo()
        {
            const string cpuInfo =
                "processor\t: 0\n" +
                "vendor_id\t: GenuineIntel\n" +
                "model name\t: 12th Gen Intel(R) Core(TM) i9-12900K\n" +
                "processor\t: 1\n" +
                "model name\t: 12th Gen Intel(R) Core(TM) i9-12900K\n";

            Assert.Equal("12th Gen Intel(R) Core(TM) i9-12900K", LinuxHardwareUsageService.ParseCpuModelName(cpuInfo));
        }
    }
}

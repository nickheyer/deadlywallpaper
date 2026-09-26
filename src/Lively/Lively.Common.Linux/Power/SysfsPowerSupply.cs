using System;
using System.IO;

namespace Lively.Common.Linux.Power
{
    /// <summary>
    /// Reads the kernel power supply class (<c>/sys/class/power_supply</c>): the machine is on battery when at least one
    /// supply of type <c>Mains</c> exists and none of them reports <c>online</c> = 1.
    /// </summary>
    public sealed class SysfsPowerSupply
    {
        public const string DefaultRoot = "/sys/class/power_supply";
        private const string MainsType = "Mains";
        private const string TypeFile = "type";
        private const string OnlineFile = "online";

        public SysfsPowerSupply(string root)
        {
            if (string.IsNullOrWhiteSpace(root))
                throw new ArgumentException("The sysfs power supply root must not be empty.", nameof(root));
            Root = root;
        }

        public string Root { get; }

        /// <summary>
        /// True when a mains supply exists and every mains supply is offline; false when no mains supply
        /// (with a readable <c>online</c> attribute) exists under <see cref="Root"/>.
        /// </summary>
        public bool IsOnBattery()
        {
            if (!Directory.Exists(Root))
                return false;

            var mainsFound = false;
            var mainsOnline = false;
            foreach (var supply in Directory.EnumerateFileSystemEntries(Root))
            {
                var typePath = Path.Combine(supply, TypeFile);
                var onlinePath = Path.Combine(supply, OnlineFile);
                if (!File.Exists(typePath) || !File.Exists(onlinePath))
                    continue;
                if (!string.Equals(ReadAttribute(typePath), MainsType, StringComparison.Ordinal))
                    continue;
                mainsFound = true;
                if (ReadAttribute(onlinePath) == "1")
                    mainsOnline = true;
            }
            return mainsFound && !mainsOnline;
        }

        private static string ReadAttribute(string path) => File.ReadAllText(path).Trim();
    }
}

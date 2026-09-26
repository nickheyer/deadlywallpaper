using Lively.Common.JsonConverters;
using Lively.Models.Message;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System;
using System.Collections.Generic;

namespace Lively.Core.Linux.Hosting
{
    /// <summary>
    /// Message extension understood only by lively-mpv-host: a raw mpv command
    /// (PROTOCOL.md section 2, type 100).
    /// </summary>
    public sealed class HostMpvCommand : IpcMessage
    {
        public const int TypeValue = 100;

        public List<object> Command { get; set; } = new List<object>();

        public HostMpvCommand() : base((MessageType)TypeValue)
        {
        }

        public HostMpvCommand(params object[] command) : this()
        {
            Command.AddRange(command);
        }
    }

    /// <summary>
    /// Serialisation of the JSON-lines protocol shared by the native hosts and the Plasma plugin.
    /// </summary>
    public static class HostProtocol
    {
        private static readonly JsonSerializerSettings ReadSettings = new JsonSerializerSettings
        {
            Converters = { new IpcMessageConverter() }
        };

        public static string Serialize(IpcMessage message)
        {
            return JsonConvert.SerializeObject(message);
        }

        /// <summary>
        /// Parses one line from a host. Returns null for lines that are not Lively messages
        /// (unknown type or not JSON) so callers can log them as plain output.
        /// </summary>
        public static IpcMessage TryParse(string line)
        {
            if (string.IsNullOrWhiteSpace(line) || line[0] != '{')
                return null;

            try
            {
                var jo = JObject.Parse(line);
                if (jo["Type"] == null)
                    return null;
                return JsonConvert.DeserializeObject<IpcMessage>(line, ReadSettings);
            }
            catch (JsonException)
            {
                return null;
            }
        }
    }
}

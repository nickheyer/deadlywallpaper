using System;

namespace Lively.Models.Message
{
    /// <summary>
    /// One audio spectrum frame (128 smoothed magnitude bins) for <c>livelyAudioListener</c>.
    /// </summary>
    [Serializable]
    public class LivelySystemAudio : IpcMessage
    {
        public double[] Data { get; set; }
        public LivelySystemAudio() : base(MessageType.lsp_audio)
        {
        }
    }
}

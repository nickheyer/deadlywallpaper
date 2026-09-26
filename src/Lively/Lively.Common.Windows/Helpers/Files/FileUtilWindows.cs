using System.Diagnostics;
using System.IO;

namespace Lively.Common.Helpers.Files
{
    public static class FileUtilWindows
    {
        /// <summary>
        /// Opens the folder in file explorer; If file path is given, file is selected.<br>
        /// Does NOT work under desktop bridge!</br>
        /// </summary>
        /// <param name="path"></param>
        public static void OpenFolder(string path)
        {
            try
            {
                ProcessStartInfo startInfo = new ProcessStartInfo
                {
                    FileName = "explorer.exe"
                };
                if (File.Exists(path))
                {
                    startInfo.Arguments = "/select, \"" + path + "\"";
                }
                else if (Directory.Exists(path))
                {
                    startInfo.Arguments = "\"" + path + "\"";
                }
                else
                {
                    throw new FileNotFoundException();
                }
                Process.Start(startInfo);
            }
            catch { }
        }
    }
}

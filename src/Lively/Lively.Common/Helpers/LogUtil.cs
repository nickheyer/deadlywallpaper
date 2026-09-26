using Lively.Common.Helpers.Archive;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace Lively.Common.Helpers
{
    public static class LogUtil
    {
        /// <summary>
        /// Returns data stored in class object file.
        /// </summary>
        /// <param name="obj"></param>
        /// <returns></returns>
        public static string PropertyList(object obj)
        {
            try
            {
                var props = obj.GetType().GetProperties();
                var sb = new StringBuilder();
                foreach (var p in props)
                {
                    sb.AppendLine(p.Name + ": " + p.GetValue(obj, null));
                }
                return sb.ToString();
            }
            catch
            {
                return "Failed to retrive properties of object.";
            }
        }

        /// <summary>
        /// Let user create archive file with all the relevant diagnostic files.
        /// </summary>
        public static void ExtractLogFiles(string savePath)
        {
            var files = new List<string>();

            if (Directory.Exists(Constants.CommonPaths.LogDir))
                files.AddRange(Directory.GetFiles(Constants.CommonPaths.LogDir, "*.*", SearchOption.TopDirectoryOnly));

            if (Directory.Exists(Constants.CommonPaths.LogDirUI))
                files.AddRange(Directory.GetFiles(Constants.CommonPaths.LogDirUI, "*.*", SearchOption.TopDirectoryOnly));

            if (File.Exists(Constants.CommonPaths.UserSettingsPath))
                files.Add(Constants.CommonPaths.UserSettingsPath);

            if (File.Exists(Constants.CommonPaths.WallpaperLayoutPath))
                files.Add(Constants.CommonPaths.WallpaperLayoutPath);

            var cefLogFile = Path.Combine(Constants.CommonPaths.TempCefDir, "logfile.txt");
            if (File.Exists(cefLogFile))
                files.Add(cefLogFile);

            /*
            var procFile = Path.Combine(Program.AppDataDir, "temp", "process.txt");
            File.WriteAllLines(procFile, Process.GetProcesses().Select(x => x.ProcessName));
            files.Add(procFile);
            */

            ZipCreate.CreateZip(savePath, new List<ZipCreate.FileData>() 
            {
                new ZipCreate.FileData() 
                {
                    ParentDirectory = Constants.CommonPaths.AppDataDir,
                    Files = files
                }
            });
        }
    }
}

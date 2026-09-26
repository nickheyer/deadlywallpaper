using System;
using System.Threading.Tasks;

namespace Lively.Common.Linux.DBus
{
    /// <summary>
    /// Runs asynchronous D-Bus work from synchronous interface members. The work is scheduled on the
    /// thread pool so its continuations never wait on the caller's synchronization context, which is what
    /// keeps a UI thread that blocks here from deadlocking with the D-Bus reply.
    /// </summary>
    public static class DBusSync
    {
        public static T Run<T>(Func<Task<T>> work)
        {
            ArgumentNullException.ThrowIfNull(work);
            return Task.Run(work).GetAwaiter().GetResult();
        }

        public static void Run(Func<Task> work)
        {
            ArgumentNullException.ThrowIfNull(work);
            Task.Run(work).GetAwaiter().GetResult();
        }
    }
}

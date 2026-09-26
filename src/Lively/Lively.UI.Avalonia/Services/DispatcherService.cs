using Avalonia.Threading;
using Lively.Common.Services;
using System;

namespace Lively.UI.Avalonia.Services
{
    public class DispatcherService : IDispatcherService
    {
        public bool TryEnqueue(Action action)
        {
            if (action is null)
                return false;

            Dispatcher.UIThread.Post(action);
            return true;
        }
    }
}

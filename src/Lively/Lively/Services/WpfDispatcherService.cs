using Lively.Common.Services;
using System;
using System.Windows;
using System.Windows.Threading;

namespace Lively.Services
{
    public class WpfDispatcherService : IDispatcherService
    {
        public bool TryEnqueue(Action action)
        {
            _ = Application.Current.Dispatcher.BeginInvoke(DispatcherPriority.Normal, action);
            return true;
        }
    }
}

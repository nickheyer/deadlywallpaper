using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.SecretService
{
    /// <summary>
    /// freedesktop Secret Service API, <c>org.freedesktop.Secret.Service</c> at <c>/org/freedesktop/secrets</c>.
    /// </summary>
    [DBusInterface("org.freedesktop.Secret.Service")]
    public interface ISecretService : IDBusObject
    {
        /// <summary>OpenSession(algorithm, input) → (output, session path)</summary>
        Task<(object output, ObjectPath result)> OpenSessionAsync(string algorithm, object input);

        /// <summary>ReadAlias(name) → collection path, or "/" when the alias is not set.</summary>
        Task<ObjectPath> ReadAliasAsync(string name);

        /// <summary>Unlock(objects) → (unlocked, prompt path or "/")</summary>
        Task<(ObjectPath[] unlocked, ObjectPath prompt)> UnlockAsync(ObjectPath[] objects);

        /// <summary>SearchItems(attributes) → (unlocked item paths, locked item paths)</summary>
        Task<(ObjectPath[] unlocked, ObjectPath[] locked)> SearchItemsAsync(IDictionary<string, string> attributes);

        /// <summary>GetSecrets(items, session) → item path → secret struct</summary>
        Task<IDictionary<ObjectPath, (ObjectPath session, byte[] parameters, byte[] value, string contentType)>> GetSecretsAsync(ObjectPath[] items, ObjectPath session);
    }

    [DBusInterface("org.freedesktop.Secret.Collection")]
    public interface ISecretCollection : IDBusObject
    {
        /// <summary>CreateItem(properties, secret, replace) → (item path or "/", prompt path or "/")</summary>
        Task<(ObjectPath item, ObjectPath prompt)> CreateItemAsync(IDictionary<string, object> properties, (ObjectPath session, byte[] parameters, byte[] value, string contentType) secret, bool replace);

        /// <summary>SearchItems(attributes) → item paths</summary>
        Task<ObjectPath[]> SearchItemsAsync(IDictionary<string, string> attributes);

        /// <summary>org.freedesktop.DBus.Properties.Get for this interface (Label, Locked, Items, Created, Modified).</summary>
        Task<object> GetAsync(string prop);
    }

    [DBusInterface("org.freedesktop.Secret.Session")]
    public interface ISecretSession : IDBusObject
    {
        Task CloseAsync();
    }

    [DBusInterface("org.freedesktop.Secret.Prompt")]
    public interface ISecretPrompt : IDBusObject
    {
        /// <summary>Prompt(window-id): shows the prompt; the outcome arrives through <see cref="WatchCompletedAsync"/>.</summary>
        Task PromptAsync(string windowId);

        Task DismissAsync();

        /// <summary>Completed(dismissed, result)</summary>
        Task<IDisposable> WatchCompletedAsync(Action<(bool dismissed, object result)> handler, Action<Exception> onError = null);
    }
}

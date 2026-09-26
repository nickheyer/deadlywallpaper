using Lively.Common.Services;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.SecretService
{
    /// <summary>
    /// Stores tokens in the user's keyring through the freedesktop Secret Service API.
    /// <see cref="Protect"/> saves the data as the secret of a new keyring item and returns the UTF-8 bytes of the
    /// item's generated id; <see cref="Unprotect"/> resolves that id back to the stored secret.
    /// </summary>
    public sealed class SecretServiceTokenProtector : ITokenProtector
    {
        public const string BusName = "org.freedesktop.secrets";
        public static readonly ObjectPath ServicePath = new("/org/freedesktop/secrets");
        public static readonly ObjectPath LoginCollectionPath = new("/org/freedesktop/secrets/collection/login");
        public const string ItemLabel = "Lively Wallpaper token";
        public const string ApplicationAttributeValue = "lively-wallpaper";
        public const string ContentType = "application/octet-stream";

        private const string ApplicationAttribute = "application";
        private const string IdAttribute = "id";
        private const string PlainAlgorithm = "plain";
        private const string DefaultAlias = "default";
        private const string NoObject = "/";
        private const string LabelProperty = "org.freedesktop.Secret.Item.Label";
        private const string AttributesProperty = "org.freedesktop.Secret.Item.Attributes";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Connection connection;

        public SecretServiceTokenProtector()
            : this(DBusConnections.Session)
        {
        }

        public SecretServiceTokenProtector(Connection connection)
        {
            this.connection = connection ?? throw new ArgumentNullException(nameof(connection));
        }

        /// <inheritdoc/>
        /// <exception cref="PlatformNotSupportedException">No Secret Service provider owns or can be activated for <c>org.freedesktop.secrets</c>.</exception>
        /// <exception cref="OperationCanceledException">The keyring prompt was dismissed.</exception>
        public byte[] Protect(byte[] data)
        {
            ArgumentNullException.ThrowIfNull(data);
            return DBusSync.Run(() => ProtectAsync(data));
        }

        /// <inheritdoc/>
        /// <exception cref="PlatformNotSupportedException">No Secret Service provider owns or can be activated for <c>org.freedesktop.secrets</c>.</exception>
        /// <exception cref="CryptographicException">No keyring item exists for the id in <paramref name="data"/>.</exception>
        /// <exception cref="OperationCanceledException">The keyring prompt was dismissed.</exception>
        public byte[] Unprotect(byte[] data)
        {
            ArgumentNullException.ThrowIfNull(data);
            return DBusSync.Run(() => UnprotectAsync(data));
        }

        private async Task<byte[]> ProtectAsync(byte[] data)
        {
            await EnsureServiceAvailableAsync();
            var service = connection.CreateProxy<ISecretService>(BusName, ServicePath);
            var session = await OpenPlainSessionAsync(service);
            try
            {
                var collectionPath = await ResolveDefaultCollectionAsync(service);
                var collection = connection.CreateProxy<ISecretCollection>(BusName, collectionPath);
                await UnlockCollectionAsync(service, collection, collectionPath);

                var id = Guid.NewGuid().ToString();
                var properties = new Dictionary<string, object>
                {
                    [LabelProperty] = ItemLabel,
                    [AttributesProperty] = CreateAttributes(id),
                };
                var secret = (session, Array.Empty<byte>(), data, ContentType);
                var (item, prompt) = await collection.CreateItemAsync(properties, secret, replace: true);
                if (IsNoObject(item))
                {
                    var result = await CompletePromptAsync(prompt);
                    item = (ObjectPath)result;
                }
                Logger.Info("Stored token {0} as keyring item {1}", id, item);
                return Encoding.UTF8.GetBytes(id);
            }
            finally
            {
                await CloseSessionAsync(session);
            }
        }

        private async Task<byte[]> UnprotectAsync(byte[] handle)
        {
            await EnsureServiceAvailableAsync();
            var id = Encoding.UTF8.GetString(handle);
            var service = connection.CreateProxy<ISecretService>(BusName, ServicePath);
            var session = await OpenPlainSessionAsync(service);
            try
            {
                var (unlocked, locked) = await service.SearchItemsAsync(CreateAttributes(id));
                var items = new List<ObjectPath>(unlocked);
                if (locked.Length > 0)
                {
                    var (nowUnlocked, prompt) = await service.UnlockAsync(locked);
                    items.AddRange(nowUnlocked);
                    if (!IsNoObject(prompt))
                        items.AddRange((ObjectPath[])await CompletePromptAsync(prompt));
                }
                items = items.Distinct().ToList();
                if (items.Count == 0)
                    throw new CryptographicException($"No keyring item exists for token id '{id}'.");

                var secrets = await service.GetSecretsAsync(items.ToArray(), session);
                foreach (var item in items)
                {
                    if (secrets.TryGetValue(item, out var secret))
                        return secret.value;
                }
                throw new CryptographicException($"The keyring returned no secret for token id '{id}'.");
            }
            finally
            {
                await CloseSessionAsync(session);
            }
        }

        private async Task EnsureServiceAvailableAsync()
        {
            if (await connection.IsServiceActiveAsync(BusName))
                return;
            var activatable = await connection.ListActivatableServicesAsync();
            if (activatable.Contains(BusName))
                return;
            throw new PlatformNotSupportedException("No Secret Service (keyring) is available on the session bus");
        }

        private static async Task<ObjectPath> OpenPlainSessionAsync(ISecretService service)
        {
            var (_, session) = await service.OpenSessionAsync(PlainAlgorithm, string.Empty);
            return session;
        }

        private async Task CloseSessionAsync(ObjectPath session)
        {
            await connection.CreateProxy<ISecretSession>(BusName, session).CloseAsync();
        }

        private static async Task<ObjectPath> ResolveDefaultCollectionAsync(ISecretService service)
        {
            var path = await service.ReadAliasAsync(DefaultAlias);
            if (!IsNoObject(path))
                return path;
            Logger.Info("The keyring has no '{0}' collection alias; using {1}", DefaultAlias, LoginCollectionPath);
            return LoginCollectionPath;
        }

        private async Task UnlockCollectionAsync(ISecretService service, ISecretCollection collection, ObjectPath collectionPath)
        {
            var locked = (bool)await collection.GetAsync("Locked");
            if (!locked)
                return;

            var (unlocked, prompt) = await service.UnlockAsync([collectionPath]);
            if (unlocked.Contains(collectionPath))
                return;
            if (IsNoObject(prompt))
                throw new CryptographicException($"The keyring collection {collectionPath} is locked and the Secret Service offered no unlock prompt.");

            var unlockedByPrompt = (ObjectPath[])await CompletePromptAsync(prompt);
            if (!unlockedByPrompt.Contains(collectionPath))
                throw new CryptographicException($"The keyring collection {collectionPath} is still locked after the unlock prompt.");
        }

        /// <summary>
        /// Shows a Secret Service prompt and returns its result variant once the <c>Completed</c> signal arrives.
        /// </summary>
        private async Task<object> CompletePromptAsync(ObjectPath promptPath)
        {
            var prompt = connection.CreateProxy<ISecretPrompt>(BusName, promptPath);
            var completion = new TaskCompletionSource<(bool dismissed, object result)>(TaskCreationOptions.RunContinuationsAsynchronously);
            using (await prompt.WatchCompletedAsync(outcome => completion.TrySetResult(outcome), error => completion.TrySetException(error)))
            {
                Logger.Info("Waiting for the keyring prompt {0}", promptPath);
                await prompt.PromptAsync(string.Empty);
                var (dismissed, result) = await completion.Task;
                if (dismissed)
                    throw new OperationCanceledException("The keyring prompt was dismissed.");
                return result;
            }
        }

        private static Dictionary<string, string> CreateAttributes(string id) => new()
        {
            [ApplicationAttribute] = ApplicationAttributeValue,
            [IdAttribute] = id,
        };

        private static bool IsNoObject(ObjectPath path) => path.ToString() == NoObject;
    }
}

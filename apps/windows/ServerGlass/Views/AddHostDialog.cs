using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using ServerGlass.Core;

namespace ServerGlass.Views;

/// <summary>
/// Adding a server.
/// </summary>
/// <remarks>
/// <para>
/// The four sign-in methods are the ones the core supports, worded for someone who is not sure
/// which they have. A desktop has both an agent and a real filesystem, so unlike the phone apps
/// this one offers a file picker rather than only a paste box — but it keeps the paste box too,
/// because a key copied out of a password manager never touches disk that way.
/// </para>
/// <para>
/// Nothing typed here is stored by this dialog. The caller writes the secret to the platform store
/// and builds the config from it; <see cref="SavedHost"/> has no field for a secret at all.
/// </para>
/// </remarks>
internal sealed class AddHostDialog : ContentDialog
{
    private readonly TextBox _address = new() { PlaceholderText = "10.0.0.4 or server.example.com" };
    private readonly NumberBox _port = new() { Value = 22, Minimum = 1, Maximum = 65535, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
    private readonly TextBox _user = new() { Text = "root" };
    private readonly ComboBox _auth = new();
    private readonly TextBox _keyPath = new() { PlaceholderText = @"C:\Users\you\.ssh\id_ed25519" };
    private readonly Button _browse = new() { Content = "Browse…" };
    private readonly TextBox _keyText = new()
    {
        AcceptsReturn = true,
        Height = 96,
        PlaceholderText = "-----BEGIN OPENSSH PRIVATE KEY-----",
        TextWrapping = TextWrapping.Wrap,
    };
    private readonly PasswordBox _secret = new();
    private readonly ComboBox _policy = new();
    private readonly NumberBox _refresh = new() { Value = 1000, Minimum = 250, Maximum = 60000, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };

    private readonly StackPanel _keyPathRow;
    private readonly StackPanel _keyTextRow;
    private readonly StackPanel _secretRow;

    public AddHostDialog()
    {
        Title = "Add a server";
        PrimaryButtonText = "Add";
        CloseButtonText = "Cancel";
        DefaultButton = ContentDialogButton.Primary;
        Theme.StyleDialog(this);

        _auth.ItemsSource = new[]
        {
            "Use my SSH agent (Pageant)",
            "A key file on this PC",
            "Paste a private key",
            "A password",
        };
        _auth.SelectedIndex = 0;
        _auth.SelectionChanged += (_, _) => RefreshFields();

        _policy.ItemsSource = new[]
        {
            "Strict — the key must already be trusted",
            "Trust on first connection",
            "Accept any key (not recommended)",
        };
        _policy.SelectedIndex = 1;

        _browse.Click += (_, _) => _ = PickKey();

        var fields = new StackPanel { Spacing = 12, Width = 420 };
        fields.Children.Add(Field("Address", _address));

        var portAndUser = new Grid { ColumnSpacing = 12 };
        portAndUser.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        portAndUser.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(2, GridUnitType.Star) });
        var portField = Field("Port", _port);
        var userField = Field("Username", _user);
        Grid.SetColumn(portField, 0);
        Grid.SetColumn(userField, 1);
        portAndUser.Children.Add(portField);
        portAndUser.Children.Add(userField);
        fields.Children.Add(portAndUser);

        fields.Children.Add(Field("Signing in with", _auth));

        var pathRow = new Grid { ColumnSpacing = 8 };
        pathRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        pathRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(_keyPath, 0);
        Grid.SetColumn(_browse, 1);
        pathRow.Children.Add(_keyPath);
        pathRow.Children.Add(_browse);

        _keyPathRow = Field("Key file", pathRow);
        _keyTextRow = Field("Private key", _keyText);
        _secretRow = Field("Password or key passphrase", _secret);

        fields.Children.Add(_keyPathRow);
        fields.Children.Add(_keyTextRow);
        fields.Children.Add(_secretRow);
        fields.Children.Add(Field("Host key", _policy));
        fields.Children.Add(Field("Refresh every (ms)", _refresh));

        Content = new ScrollViewer
        {
            Content = fields,
            MaxHeight = 560,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };

        RefreshFields();
        Opened += (_, _) => _address.Focus(FocusState.Programmatic);
        PrimaryButtonClick += OnPrimary;
    }

    /// <summary>The record to save. Valid only when the dialog closed with Add.</summary>
    public SavedHost Result { get; private set; } = new();

    /// <summary>The password or passphrase, if one was typed. Never part of <see cref="Result"/>.</summary>
    public string? Secret { get; private set; }

    /// <summary>A pasted private key, if one was given. Never part of <see cref="Result"/>.</summary>
    public string? KeyText { get; private set; }

    private static StackPanel Field(string label, UIElement control)
    {
        var stack = new StackPanel { Spacing = 4 };
        stack.Children.Add(Widgets.Label(label, 11.5));
        stack.Children.Add((FrameworkElement)control);
        return stack;
    }

    private string AuthKind => _auth.SelectedIndex switch
    {
        1 => "key",
        2 => "key_text",
        3 => "password",
        _ => "agent",
    };

    private string Policy => _policy.SelectedIndex switch
    {
        0 => "strict",
        2 => "accept_any",
        _ => "accept_new",
    };

    private void RefreshFields()
    {
        _keyPathRow.Visibility = AuthKind == "key" ? Visibility.Visible : Visibility.Collapsed;
        _keyTextRow.Visibility = AuthKind == "key_text" ? Visibility.Visible : Visibility.Collapsed;

        // A passphrase belongs with either kind of key; a password only with password auth.
        _secretRow.Visibility = AuthKind == "agent" ? Visibility.Collapsed : Visibility.Visible;
        if (_secretRow.Children[0] is TextBlock label)
        {
            label.Text = AuthKind == "password" ? "Password" : "Key passphrase (if it has one)";
        }
    }

    private async Task PickKey()
    {
        // An unpackaged app has no implicit window for a picker to attach to. Without an explicit
        // owner the picker throws — at runtime, on the click, which is the worst place to find out.
        // The path field stays typeable either way, so a missing handle degrades rather than blocks.
        if (MainWindowHandle == IntPtr.Zero)
        {
            _keyPath.Focus(FocusState.Programmatic);
            return;
        }

        var picker = new Windows.Storage.Pickers.FileOpenPicker();
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, MainWindowHandle);

        var file = await picker.PickSingleFileAsync();
        if (file is not null)
        {
            _keyPath.Text = file.Path;
        }
    }

    /// <summary>The window a file picker attaches to. Set once by the main window.</summary>
    internal static IntPtr MainWindowHandle { get; set; }

    private void OnPrimary(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        var address = _address.Text.Trim();
        if (address.Length == 0)
        {
            args.Cancel = true;
            _address.Focus(FocusState.Programmatic);
            return;
        }

        Result = new SavedHost
        {
            Address = address,
            Port = (ushort)Math.Clamp(double.IsNaN(_port.Value) ? 22 : _port.Value, 1, 65535),
            User = string.IsNullOrWhiteSpace(_user.Text) ? "root" : _user.Text.Trim(),
            AuthKind = AuthKind,
            KeyPath = AuthKind == "key" && _keyPath.Text.Trim().Length > 0 ? _keyPath.Text.Trim() : null,
            HostKeyPolicy = Policy,
            RefreshMs = (ulong)Math.Clamp(double.IsNaN(_refresh.Value) ? 1000 : _refresh.Value, 250, 60000),
        };

        Secret = _secret.Password.Length > 0 ? _secret.Password : null;
        KeyText = AuthKind == "key_text" && _keyText.Text.Trim().Length > 0 ? _keyText.Text : null;
    }
}

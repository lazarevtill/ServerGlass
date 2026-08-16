using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using QRCoder;
using ServerGlass.Core;

namespace ServerGlass.Views;

/// <summary>
/// Moving a host inventory between devices.
/// </summary>
/// <remarks>
/// <para>
/// Not a sync service: there is no server, no account, and nothing persists anywhere between the
/// two devices. One device shows a QR and the other scans it. The code carries a <em>public</em>
/// key, a session nonce and the addresses this device might answer at — never a shared secret,
/// because a screen can be photographed and a screenshot of a QR is as good as the original.
/// </para>
/// <para>
/// Both sides derive the same six-digit code from the full transcript. The user compares the two
/// screens, and only then does anything transfer. That is why receiving is two calls and not one.
/// </para>
/// <para>
/// <b>A desktop has no camera</b>, so scanning is not offered. It can show a QR to receive an
/// inventory, and it can take a pairing code pasted from the other device in order to send one.
/// </para>
/// <para>
/// What crosses: host records and <c>known_hosts</c> lines. What does not: passwords, passphrases
/// and pasted keys — they are not fields of the wire format at all, and the receiving device asks
/// for each credential once and keeps it in its own store.
/// </para>
/// </remarks>
internal sealed class PairingDialog : ContentDialog
{
    private readonly CoreModel _model;
    private readonly StackPanel _body = new() { Spacing = 14, Width = 420 };
    private ulong _session;

    public PairingDialog(CoreModel model)
    {
        _model = model;
        Title = "Copy servers between devices";
        CloseButtonText = "Close";
        Theme.StyleDialog(this);
        Content = new ScrollViewer
        {
            Content = _body,
            MaxHeight = 620,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };

        ShowChoice();
        Closed += (_, _) =>
        {
            if (_session != 0)
            {
                _model.ForgetPairing(_session);
            }
        };
    }

    private void Explain(string text, double size = 12, bool dim = true)
    {
        _body.Children.Add(new TextBlock
        {
            Text = text,
            FontSize = size,
            Foreground = Theme.Brush(dim ? Theme.Secondary : Theme.Primary),
            TextWrapping = TextWrapping.Wrap,
        });
    }

    private void ShowChoice()
    {
        _body.Children.Clear();
        Explain("Nothing is uploaded anywhere. The two devices talk to each other directly, on "
                + "your own network, and the transfer only happens after you have checked that "
                + "both screens show the same six digits.");

        Explain("Passwords and private keys never travel. Only the list of servers and the host "
                + "keys you have already trusted. This device will ask for each sign-in once.");

        var receive = new Button
        {
            Content = "Receive servers from another device",
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        receive.Click += (_, _) => _ = StartReceiving();

        var send = new Button
        {
            Content = "Send my servers to another device",
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        send.Click += (_, _) => ShowSend();

        _body.Children.Add(receive);
        _body.Children.Add(send);
    }

    // -----------------------------------------------------------------------------------------
    // Receiving
    // -----------------------------------------------------------------------------------------

    private async Task StartReceiving()
    {
        _body.Children.Clear();
        Explain("Starting…");

        var addresses = CoreModel.LocalAddresses();
        if (addresses.Count == 0)
        {
            _body.Children.Clear();
            Explain("This PC has no network address other than its own loopback, so another "
                    + "device has no way to reach it. Connect to a network and try again.", 12, false);
            return;
        }

        ReceiverStarted started;
        try
        {
            started = await _model.StartReceiving(addresses);
        }
        catch (SgException error)
        {
            _body.Children.Clear();
            Explain($"Pairing could not start: {error.Message}", 12, false);
            return;
        }

        _session = started.Id;

        _body.Children.Clear();
        Explain("Scan this with the other device.", 13, false);
        _body.Children.Add(QrView.Render(started.PairingCode, 240));
        Explain($"Reachable at: {string.Join(", ", addresses)}", 10.5);
        Explain("Waiting for the other device…");

        string code;
        try
        {
            // Blocks until the other device connects. Nothing has been received when it returns.
            code = await _model.AwaitPairing(started.Id);
        }
        catch (SgException error)
        {
            _body.Children.Clear();
            Explain($"Pairing stopped: {error.Message}", 12, false);
            return;
        }

        ConfirmThenReceive(code);
    }

    private void ConfirmThenReceive(string code)
    {
        _body.Children.Clear();
        Explain("Check that the other device shows these same six digits.", 13, false);
        _body.Children.Add(VerificationCode(code));
        Explain("If they do not match, close this — something is answering that should not be.");

        var accept = new Button
        {
            Content = "They match — copy the servers",
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        accept.Click += (_, _) => _ = Receive();
        _body.Children.Add(accept);
    }

    private async Task Receive()
    {
        SyncBundle incoming;
        try
        {
            incoming = await _model.ReceivePairing(_session);
        }
        catch (SgException error)
        {
            _body.Children.Clear();
            Explain($"The transfer did not finish: {error.Message}", 12, false);
            return;
        }

        var existing = CurrentBundle();
        var merged = _model.MergeBundle(existing, incoming);

        // The merge decides; it does not store. Writing the result is this layer's job.
        var known = new HashSet<string>(existing.Hosts.Select(Key));
        foreach (var host in merged.Hosts.Where(h => !known.Contains(Key(h))))
        {
            _model.Add(
                new SavedHost
                {
                    Address = host.Address,
                    Port = host.Port,
                    User = host.User,
                    AuthKind = host.AuthKind,
                    KeyPath = host.KeyPath,
                    HostKeyPolicy = host.HostKeyPolicy,
                    RefreshMs = host.RefreshMs,
                },
                secret: null,
                keyText: null);
        }

        File.WriteAllLines(_model.Store.KnownHostsPath, merged.KnownHosts);

        _body.Children.Clear();
        Explain($"Copied {merged.AddedHosts} server(s) and {merged.AddedPins} trusted host key(s). "
                + $"{merged.KeptHosts} you already had were left alone.", 13, false);

        if (merged.AddedHosts > 0)
        {
            Explain("Each copied server still needs its sign-in details entering here once — a "
                    + "password or key never leaves the device it was typed on.");
        }

        if (merged.Conflicts.Count > 0)
        {
            ShowConflicts(merged.Conflicts);
        }
    }

    /// <summary>
    /// Host keys the two devices disagree about.
    /// </summary>
    /// <remarks>
    /// Reported and never applied. A sync channel that can quietly rewrite a pin is a
    /// machine-in-the-middle with extra steps.
    /// </remarks>
    private void ShowConflicts(IReadOnlyList<PinConflictView> conflicts)
    {
        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(new TextBlock
        {
            Text = $"{conflicts.Count} host key(s) disagree and were NOT changed",
            FontSize = 12,
            FontWeight = FontWeights.SemiBold,
            Foreground = Theme.Brush(Theme.Warn),
            TextWrapping = TextWrapping.Wrap,
        });

        foreach (var conflict in conflicts)
        {
            stack.Children.Add(Widgets.Value(conflict.Host, 11, Theme.Primary));
            stack.Children.Add(new TextBlock
            {
                Text = "This device and the other one have different identities recorded for this "
                       + "server. That can mean it was rebuilt — or that something is impersonating "
                       + "it. Your existing record was kept.",
                FontSize = 10.5,
                Foreground = Theme.Brush(Theme.Secondary),
                TextWrapping = TextWrapping.Wrap,
            });
        }

        _body.Children.Add(new Border
        {
            Background = Theme.Tint(Theme.Warn, 0.12),
            BorderBrush = Theme.Tint(Theme.Warn, 0.35),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10),
            Child = stack,
        });
    }

    // -----------------------------------------------------------------------------------------
    // Sending
    // -----------------------------------------------------------------------------------------

    private void ShowSend()
    {
        _body.Children.Clear();
        Explain("This PC has no camera, so paste the pairing code from the other device's screen.",
                13, false);

        var input = new TextBox
        {
            AcceptsReturn = true,
            Height = 96,
            FontFamily = Theme.Mono,
            FontSize = 11,
            TextWrapping = TextWrapping.Wrap,
            PlaceholderText = "sg1:…",
        };
        _body.Children.Add(input);

        var connect = new Button
        {
            Content = "Connect",
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        connect.Click += (_, _) => _ = Connect(input.Text.Trim());
        _body.Children.Add(connect);
    }

    private async Task Connect(string code)
    {
        if (code.Length == 0)
        {
            return;
        }

        _body.Children.Clear();
        Explain("Connecting…");

        SenderConnected connected;
        try
        {
            connected = await _model.ScanPairingCode(code);
        }
        catch (SgException error)
        {
            _body.Children.Clear();
            Explain($"That code did not work: {error.Message}", 12, false);
            return;
        }

        _session = connected.Id;

        _body.Children.Clear();
        Explain("Check that the other device shows these same six digits.", 13, false);
        _body.Children.Add(VerificationCode(connected.VerificationCode));
        Explain("Nothing has been sent yet.");

        var send = new Button
        {
            Content = "They match — send the servers",
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        send.Click += (_, _) => _ = Send();
        _body.Children.Add(send);
    }

    private async Task Send()
    {
        try
        {
            await _model.SendPairing(_session, CurrentBundle());
        }
        catch (SgException error)
        {
            _body.Children.Clear();
            Explain($"The transfer did not finish: {error.Message}", 12, false);
            return;
        }

        _body.Children.Clear();
        Explain("Sent. The other device will ask for each sign-in itself — no password or key left "
                + "this PC.", 13, false);
    }

    // -----------------------------------------------------------------------------------------

    private SyncBundle CurrentBundle() => new()
    {
        Hosts =
        [
            .. _model.Hosts.Select(h => new SyncHostView
            {
                Address = h.Saved.Address,
                Port = h.Saved.Port,
                User = h.Saved.User,
                AuthKind = h.Saved.AuthKind,
                KeyPath = h.Saved.KeyPath,
                HostKeyPolicy = h.Saved.HostKeyPolicy,
                RefreshMs = h.Saved.RefreshMs,
            }),
        ],
        KnownHosts = File.Exists(_model.Store.KnownHostsPath)
            ? File.ReadAllLines(_model.Store.KnownHostsPath)
            : [],
    };

    private static string Key(SyncHostView host) => $"{host.User}@{host.Address}:{host.Port}";

    /// <summary>The six digits, large enough to read across a desk from two screens at once.</summary>
    private static UIElement VerificationCode(string code) => new Border
    {
        Background = Theme.Brush(Theme.Card),
        BorderBrush = Theme.Brush(Theme.PanelBorder),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(12),
        Padding = new Thickness(16),
        HorizontalAlignment = HorizontalAlignment.Center,
        Child = new TextBlock
        {
            Text = code,
            FontFamily = Theme.Mono,
            FontSize = 34,
            FontWeight = FontWeights.SemiBold,
            CharacterSpacing = 180,
            Foreground = Theme.Brush(Theme.Primary),
        },
    };
}

/// <summary>
/// A QR code, drawn as rectangles.
/// </summary>
/// <remarks>
/// QRCoder produces the module matrix; the drawing is ours. That keeps an imaging stack, a bitmap
/// encoder and a temporary file out of the picture, and the result is resolution-independent, which
/// matters because the thing scanning it is a phone camera pointed at a monitor.
/// </remarks>
internal static class QrView
{
    public static UIElement Render(string text, double size)
    {
        using var generator = new QRCodeGenerator();
        // Q rather than L: a monitor adds glare and moiré, and the extra redundancy costs a few
        // modules of density in return for scanning first time.
        using var data = generator.CreateQrCode(text, QRCodeGenerator.ECCLevel.Q);

        var modules = data.ModuleMatrix.Count;
        var scale = size / modules;

        var canvas = new Canvas
        {
            Width = size,
            Height = size,
            Background = new SolidColorBrush(Microsoft.UI.Colors.White),
        };

        for (var row = 0; row < modules; row++)
        {
            for (var column = 0; column < modules; column++)
            {
                if (!data.ModuleMatrix[row][column])
                {
                    continue;
                }

                // A hair of overlap: adjacent modules drawn at exact size leave sub-pixel seams
                // that a camera reads as a broken pattern.
                var block = new Rectangle
                {
                    Width = scale + 0.5,
                    Height = scale + 0.5,
                    Fill = new SolidColorBrush(Microsoft.UI.Colors.Black),
                };
                Canvas.SetLeft(block, column * scale);
                Canvas.SetTop(block, row * scale);
                canvas.Children.Add(block);
            }
        }

        // The quiet zone is part of the specification, and a QR against a dark dashboard with no
        // margin does not scan.
        return new Border
        {
            Background = new SolidColorBrush(Microsoft.UI.Colors.White),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(12),
            HorizontalAlignment = HorizontalAlignment.Center,
            Child = canvas,
        };
    }
}

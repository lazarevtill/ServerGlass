using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Shapes;
using ServerGlass.Core;
using ServerGlass.Views;

namespace ServerGlass;

public sealed partial class MainWindow : Window
{
    private readonly CoreModel _model;
    private Host? _selected;

    public MainWindow()
    {
        InitializeComponent();

        Title = "ServerGlass";
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1240, 860));

        // WinUI draws its own title bar, and it does not read the executable's icon resource — the
        // one <ApplicationIcon> embeds, which the taskbar, Alt-Tab and the Start Menu shortcut all
        // pick up. Without this the bar shows the generic placeholder while every other surface
        // shows the real mark, which looks like a half-finished app rather than a missing setting.
        // SetIcon wants a file, so the .ico ships beside the executable as well as inside it;
        // scripts/package-windows.ps1 checks it is in the payload.
        // Qualified: Microsoft.UI.Xaml.Shapes.Path is in scope here too.
        AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "ServerGlass.ico"));

        // The title bar does not follow the app's requested theme on its own, so a dark app gets a
        // light bar bolted to the top of it. Painted to match the sidebar it sits above.
        var bar = AppWindow.TitleBar;
        bar.BackgroundColor = Theme.Panel;
        bar.ForegroundColor = Theme.Primary;
        bar.InactiveBackgroundColor = Theme.Panel;
        bar.InactiveForegroundColor = Theme.Secondary;
        bar.ButtonBackgroundColor = Theme.Panel;
        bar.ButtonForegroundColor = Theme.Primary;
        bar.ButtonInactiveBackgroundColor = Theme.Panel;
        bar.ButtonInactiveForegroundColor = Theme.Secondary;
        bar.ButtonHoverBackgroundColor = Theme.Card;
        bar.ButtonHoverForegroundColor = Theme.Primary;

        // An unpackaged app has to hand a file picker an owner window explicitly.
        AddHostDialog.MainWindowHandle = WinRT.Interop.WindowNative.GetWindowHandle(this);

        Root.Background = Theme.Brush(Theme.Background);
        Sidebar.Background = Theme.Brush(Theme.Panel);
        AppTitle.Foreground = Theme.Brush(Theme.Primary);
        AppTitle.FontFamily = Theme.Ui;

        _model = new CoreModel(DispatcherQueue);
        _model.Problem += (_, message) => _ = ShowProblem(message);
        _model.Hosts.CollectionChanged += (_, _) => RebuildSidebar();

        RebuildSidebar();
        Closed += (_, _) => _model.Dispose();
    }

    private void RebuildSidebar()
    {
        HostItems.Children.Clear();
        foreach (var host in _model.Hosts)
        {
            var row = new HostRow(host);
            row.Click += (_, _) => Select(host);
            HostItems.Children.Add(row);
        }

        if (_model.Hosts.Count == 0)
        {
            _selected = null;
            Detail.Children.Clear();
            Detail.Children.Add(EmptyState.Build(() => _ = AddServer()));
            return;
        }

        // Keep the current selection across a list rebuild; fall back to the first host.
        Select(_model.Hosts.Contains(_selected!) ? _selected! : _model.Hosts[0]);
    }

    private void Select(Host host)
    {
        foreach (var row in HostItems.Children.OfType<HostRow>())
        {
            row.SetSelected(ReferenceEquals(row.Host, host));
        }

        if (ReferenceEquals(_selected, host) && Detail.Children.Count > 0)
        {
            return;
        }

        _selected = host;
        Detail.Children.Clear();
        Detail.Children.Add(new HostPage(_model, host, remove: () => _model.Remove(host)));
    }

    private void OnAddServer(object sender, RoutedEventArgs e) => _ = AddServer();

    private void OnPair(object sender, RoutedEventArgs e) => _ = Pair();

    private async Task AddServer()
    {
        var dialog = new AddHostDialog { XamlRoot = Root.XamlRoot };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary)
        {
            return;
        }

        try
        {
            Select(_model.Add(dialog.Result, dialog.Secret, dialog.KeyText));
        }
        catch (Exception error) when (error is SgException or InvalidOperationException)
        {
            await ShowProblem(error.Message);
        }
    }

    private async Task Pair()
    {
        var dialog = new PairingDialog(_model) { XamlRoot = Root.XamlRoot };
        await dialog.ShowAsync();
    }

    private async Task ShowProblem(string message)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = Root.XamlRoot,
            Title = "Something went wrong",
            Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap },
            CloseButtonText = "OK",
        };
        Theme.StyleDialog(dialog);
        await dialog.ShowAsync();
    }
}

/// <summary>
/// One sidebar row: a status dot, the name, and the plain-language verdict beneath it.
/// </summary>
/// <remarks>
/// The subtitle is the core's own sentence rather than a state name. "Everything looks good" is
/// what someone wants at a glance; "Online" only says the socket is open.
/// </remarks>
internal sealed class HostRow : Button
{
    private readonly Ellipse _dot = new() { Width = 8, Height = 8, Margin = new Thickness(0, 0, 10, 0) };
    private readonly TextBlock _title = new() { FontSize = 13.5, TextTrimming = TextTrimming.CharacterEllipsis };
    private readonly TextBlock _subtitle = new() { FontSize = 11, TextTrimming = TextTrimming.CharacterEllipsis };

    public HostRow(Host host)
    {
        Host = host;

        HorizontalAlignment = HorizontalAlignment.Stretch;
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        Padding = new Thickness(10, 8, 10, 8);
        BorderThickness = new Thickness(0);
        Background = Theme.Brush(Windows.UI.Color.FromArgb(0, 0, 0, 0));
        CornerRadius = new CornerRadius(7);

        _dot.VerticalAlignment = VerticalAlignment.Center;
        _title.Foreground = Theme.Brush(Theme.Primary);
        _title.FontFamily = Theme.Ui;
        _subtitle.Foreground = Theme.Brush(Theme.Secondary);

        var text = new StackPanel { Spacing = 1 };
        text.Children.Add(_title);
        text.Children.Add(_subtitle);

        var layout = new Grid();
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(_dot, 0);
        Grid.SetColumn(text, 1);
        layout.Children.Add(_dot);
        layout.Children.Add(text);
        Content = layout;

        host.PropertyChanged += (_, _) => Refresh();
        Refresh();
    }

    public Host Host { get; }

    public void SetSelected(bool selected) =>
        Background = selected
            ? Theme.Tint(Theme.Info, 0.14)
            : Theme.Brush(Windows.UI.Color.FromArgb(0, 0, 0, 0));

    private void Refresh()
    {
        _title.Text = Host.Title;
        _subtitle.Text = Host.Subtitle;
        _dot.Fill = Theme.LevelBrush(Host.StatusLevel);
    }
}

/// <summary>What the window shows before there is anything to monitor.</summary>
internal static class EmptyState
{
    public static UIElement Build(Action addServer)
    {
        var panel = new StackPanel
        {
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            Spacing = 10,
            MaxWidth = 420,
        };

        panel.Children.Add(new TextBlock
        {
            Text = "No servers yet",
            FontSize = 22,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            FontFamily = Theme.Ui,
            Foreground = Theme.Brush(Theme.Primary),
            HorizontalAlignment = HorizontalAlignment.Center,
        });

        panel.Children.Add(new TextBlock
        {
            Text = "ServerGlass watches a Linux server over SSH. It installs nothing on the "
                   + "machine it is watching, and it only ever reads.",
            FontSize = 13,
            Foreground = Theme.Brush(Theme.Secondary),
            TextWrapping = TextWrapping.Wrap,
            TextAlignment = TextAlignment.Center,
        });

        var button = new Button
        {
            Content = "Add a server",
            HorizontalAlignment = HorizontalAlignment.Center,
            Margin = new Thickness(0, 8, 0, 0),
        };
        button.Click += (_, _) => addServer();
        panel.Children.Add(button);

        return panel;
    }
}

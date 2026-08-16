using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using ServerGlass.Core;

namespace ServerGlass.Views;

/// <summary>
/// One host: a header, the choice of how much detail to show, and the view itself.
/// </summary>
/// <remarks>
/// The plain screen is the default and the dense one is one click away, with the choice remembered
/// for the session. Density is right for someone triaging a server and wrong as a default — that
/// ordering is the whole argument of DESIGN.md's "density is earned".
/// </remarks>
internal sealed class HostPage : UserControl
{
    private readonly Host _host;

    private readonly TextBlock _name = new();
    private readonly TextBlock _subtitle = new();
    private readonly TextBlock _roundTrips = new();
    private readonly Microsoft.UI.Xaml.Shapes.Ellipse _dot = new() { Width = 8, Height = 8 };

    private readonly StackPanel _banners = new() { Spacing = 8 };
    private readonly PlainView _plain;
    private readonly TechnicalView _technical;
    private readonly CommandView _command;
    private readonly ScrollViewer _scroll = new()
    {
        VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        Padding = new Thickness(0, 0, 6, 0),
    };

    /// <summary>Remembered for the session, like the other platforms remember it.</summary>
    private static string _mode = "plain";

    public HostPage(CoreModel model, Host host, Action remove)
    {
        _host = host;
        _plain = new PlainView(model, () => ShowMode("technical"));
        _technical = new TechnicalView(model);
        _command = new CommandView(model, host);

        var root = new Grid { Padding = new Thickness(16, 14, 10, 14) };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var header = BuildHeader(remove);
        Grid.SetRow(header, 0);
        root.Children.Add(header);

        _banners.Margin = new Thickness(0, 10, 0, 0);
        Grid.SetRow(_banners, 1);
        root.Children.Add(_banners);

        _scroll.Margin = new Thickness(0, 12, 0, 0);
        Grid.SetRow(_scroll, 2);
        root.Children.Add(_scroll);

        Content = root;

        ShowMode(_mode);
        host.PropertyChanged += (_, _) => Refresh();
        Refresh();
    }

    private FrameworkElement BuildHeader(Action remove)
    {
        _name.FontSize = 19;
        _name.FontWeight = FontWeights.SemiBold;
        _name.FontFamily = Theme.Ui;
        _name.Foreground = Theme.Brush(Theme.Primary);
        _name.TextTrimming = TextTrimming.CharacterEllipsis;

        _subtitle.FontSize = 10;
        _subtitle.FontFamily = Theme.Mono;
        _subtitle.Foreground = Theme.Brush(Theme.Secondary);
        // Trimmed, and the reason is not cosmetic. A TextBlock with no trimming reports its full
        // text width as its desired width, so a long subtitle — "Debian GNU/Linux 12 (bookworm) ·
        // 6.18.33.2-microsoft-standard-WSL2 · x86_64" is a normal one — widens the star column past
        // the header and pushes the view buttons off the right edge of the window entirely.
        _subtitle.TextTrimming = TextTrimming.CharacterEllipsis;

        var identity = new StackPanel { Spacing = 3 };
        var titleRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7 };
        _dot.VerticalAlignment = VerticalAlignment.Center;
        titleRow.Children.Add(_dot);
        titleRow.Children.Add(_name);
        identity.Children.Add(titleRow);
        identity.Children.Add(_subtitle);

        // Making the round-trip count visible keeps the app's central design claim honest: it rises
        // by exactly one per refresh, however many collectors are enabled.
        _roundTrips.FontSize = 11;
        _roundTrips.FontFamily = Theme.Mono;
        _roundTrips.Foreground = Theme.Brush(Theme.Secondary);
        _roundTrips.HorizontalAlignment = HorizontalAlignment.Right;

        var counter = new StackPanel { Spacing = 1, HorizontalAlignment = HorizontalAlignment.Right };
        counter.Children.Add(_roundTrips);
        var counterLabel = Widgets.Label("ROUND TRIPS", 8.5);
        counterLabel.Foreground = Theme.Brush(Theme.Tertiary);
        counterLabel.HorizontalAlignment = HorizontalAlignment.Right;
        counter.Children.Add(counterLabel);

        var modes = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        modes.Children.Add(ModeButton("Overview", "plain"));
        modes.Children.Add(ModeButton("Every reading", "technical"));
        modes.Children.Add(ModeButton("Run a command", "command"));

        var forget = new Button { Content = "Forget", Margin = new Thickness(10, 0, 0, 0) };
        forget.Click += (_, _) => remove();

        var right = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 14,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
        };
        right.Children.Add(counter);
        right.Children.Add(modes);
        right.Children.Add(forget);

        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(identity, 0);
        Grid.SetColumn(right, 1);
        grid.Children.Add(identity);
        grid.Children.Add(right);
        return grid;
    }

    private readonly List<(Button Button, string Mode)> _modeButtons = [];

    private Button ModeButton(string text, string mode)
    {
        var button = new Button { Content = text };
        button.Click += (_, _) => ShowMode(mode);
        _modeButtons.Add((button, mode));
        return button;
    }

    private void ShowMode(string mode)
    {
        _mode = mode;
        foreach (var (button, its) in _modeButtons)
        {
            button.Background = its == mode
                ? Theme.Tint(Theme.Info, 0.20)
                : Theme.Brush(Theme.Panel);
        }

        _scroll.Content = mode switch
        {
            "technical" => _technical,
            "command" => _command,
            _ => _plain,
        };

        Refresh();
    }

    private void Refresh()
    {
        var snapshot = _host.Snapshot;

        _name.Text = _host.Title;
        _dot.Fill = Theme.LevelBrush(_host.StatusLevel);

        var parts = new[] { snapshot.Distro, snapshot.Kernel, snapshot.Arch }
            .Where(p => !string.IsNullOrEmpty(p))
            .ToArray();
        _subtitle.Text = parts.Length == 0 ? _host.Saved.Address : string.Join("  ·  ", parts);
        _roundTrips.Text = snapshot.RoundTrips.ToString();

        _banners.Children.Clear();
        if (snapshot.State.Kind == "failed")
        {
            _banners.Children.Add(Widgets.Banner(
                snapshot.State.Message ?? "Can't reach this server",
                snapshot.State.Recoverable == true
                    ? "ServerGlass will keep retrying."
                    : "This will not resolve on its own.",
                Theme.Bad));
        }

        if (snapshot.SourceErrors.Count > 0)
        {
            _banners.Children.Add(Widgets.Banner(
                $"{snapshot.SourceErrors.Count} collector(s) reported a problem",
                string.Join("\n", snapshot.SourceErrors),
                Theme.Warn));
        }

        switch (_mode)
        {
            case "technical":
                _technical.Update(snapshot);
                break;
            case "command":
                break;
            default:
                _plain.Update(snapshot, _host.Saved.Address);
                break;
        }
    }
}

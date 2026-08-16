using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using ServerGlass.Core;
using Windows.System;

namespace ServerGlass.Views;

/// <summary>
/// Running a command on the server.
/// </summary>
/// <remarks>
/// Honest about what it is: a command runner, not a terminal. There is no PTY behind it, so
/// <c>top</c>, <c>vim</c> and anything that prompts will hang rather than work — and the screen says
/// so instead of leaving someone to discover it by waiting out the sixty-second timeout. What it
/// does do is the thing people actually reach for: <c>systemctl restart nginx</c>, <c>df -h</c>,
/// <c>docker ps</c>, <c>tail -n 50 /var/log/syslog</c>.
///
/// It runs on the same connection the readings use, so there is no second sign-in and no second
/// session for the host to log.
/// </remarks>
internal sealed class CommandView : UserControl
{
    private readonly CoreModel _model;
    private readonly Host _host;

    private readonly StackPanel _transcript = new() { Spacing = 14, Padding = new Thickness(14) };
    private readonly ScrollViewer _scroll = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly TextBox _input = new();
    private readonly Button _run = new() { Content = "Run" };
    private readonly ProgressRing _busy = new() { Width = 18, Height = 18, IsActive = true, Visibility = Visibility.Collapsed };
    private readonly TextBlock _offline = new();

    private bool _running;

    public CommandView(CoreModel model, Host host)
    {
        _model = model;
        _host = host;

        _scroll.Content = _transcript;
        ShowPlaceholder();

        _input.PlaceholderText = "command";
        _input.FontFamily = Theme.Mono;
        _input.FontSize = 13;
        _input.KeyDown += OnKey;

        _run.Click += (_, _) => _ = Run();

        _offline.Text = "Not connected — commands need a live connection.";
        _offline.FontSize = 10.5;
        _offline.Foreground = Theme.Brush(Theme.Warn);
        _offline.Margin = new Thickness(0, 0, 0, 6);

        var prompt = Widgets.Value("$", 13, Theme.Good, FontWeights.SemiBold);
        prompt.VerticalAlignment = VerticalAlignment.Center;

        var row = new Grid { ColumnSpacing = 9 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(prompt, 0);
        Grid.SetColumn(_input, 1);
        row.Children.Add(prompt);
        row.Children.Add(_input);

        var trailing = new Grid();
        trailing.Children.Add(_run);
        trailing.Children.Add(_busy);
        Grid.SetColumn(trailing, 2);
        row.Children.Add(trailing);

        var inputArea = new StackPanel { Padding = new Thickness(12) };
        inputArea.Children.Add(_offline);
        inputArea.Children.Add(row);

        var bar = new Border
        {
            Background = Theme.Brush(Theme.Panel),
            BorderBrush = Theme.Brush(Theme.PanelBorder),
            BorderThickness = new Thickness(0, 1, 0, 0),
            Child = inputArea,
        };

        var root = new Grid();
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        Grid.SetRow(_scroll, 0);
        Grid.SetRow(bar, 1);
        root.Children.Add(_scroll);
        root.Children.Add(bar);

        // The command area fills the page rather than scrolling with it, so the input stays put.
        MinHeight = 460;
        Content = root;

        host.PropertyChanged += (_, _) => RefreshAvailability();
        RefreshAvailability();
    }

    private bool IsOnline => _host.Snapshot.State.IsOnline;

    private void RefreshAvailability()
    {
        var usable = IsOnline && !_running;
        _input.IsEnabled = usable;
        _run.IsEnabled = usable && !string.IsNullOrWhiteSpace(_input.Text);
        _offline.Visibility = IsOnline ? Visibility.Collapsed : Visibility.Visible;

        // The host's real name only arrives with the first snapshot, so the placeholder starts out
        // showing the address it was configured with and catches up here.
        if (_placeholderHeading is not null)
        {
            _placeholderHeading.Text = $"Run a command on {_host.Title}";
        }
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter)
        {
            e.Handled = true;
            _ = Run();
        }
        else
        {
            // Enable the button as soon as there is something to run.
            DispatcherQueue.TryEnqueue(RefreshAvailability);
        }
    }

    /// <summary>The placeholder heading, kept so it can follow the host's name once it is known.</summary>
    private TextBlock? _placeholderHeading;

    private void ShowPlaceholder()
    {
        var stack = new StackPanel { Spacing = 7, HorizontalAlignment = HorizontalAlignment.Left };

        _placeholderHeading = new TextBlock
        {
            Text = $"Run a command on {_host.Title}",
            FontSize = 13,
            FontWeight = FontWeights.Medium,
            FontFamily = Theme.Ui,
            Foreground = Theme.Brush(Theme.Primary),
        };
        stack.Children.Add(_placeholderHeading);

        stack.Children.Add(new TextBlock
        {
            Text = "It runs on the connection ServerGlass already has open. Programs that need a "
                   + "terminal of their own — top, vim, anything that asks a question — will not "
                   + "work here.",
            FontSize = 11.5,
            Foreground = Theme.Brush(Theme.Secondary),
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 560,
            // Left, explicitly. A stretched element with a MaxWidth is *centred* in the space it
            // did not use, which put this paragraph in the middle of the page beside its heading.
            HorizontalAlignment = HorizontalAlignment.Left,
        });

        var suggestions = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            Margin = new Thickness(0, 4, 0, 0),
        };

        foreach (var suggestion in new[] { "df -h", "docker ps", "uptime" })
        {
            var chip = new Button
            {
                Content = Widgets.Value(suggestion, 11, Theme.Secondary, FontWeights.Normal),
                Background = Theme.Brush(Theme.Card),
                BorderBrush = Theme.Brush(Theme.PanelBorder),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(12),
                Padding = new Thickness(9, 5, 9, 5),
            };
            var text = suggestion;
            chip.Click += (_, _) =>
            {
                _input.Text = text;
                RefreshAvailability();
                _input.Focus(FocusState.Programmatic);
            };
            suggestions.Children.Add(chip);
        }

        stack.Children.Add(suggestions);
        _transcript.Children.Add(stack);
    }

    private async Task Run()
    {
        var typed = _input.Text.Trim();
        if (typed.Length == 0 || !IsOnline || _running)
        {
            return;
        }

        if (_transcript.Children.Count == 1 && _transcript.Children[0] is StackPanel { Spacing: 7 })
        {
            _transcript.Children.Clear();
        }

        _input.Text = "";
        _running = true;
        _busy.Visibility = Visibility.Visible;
        _run.Visibility = Visibility.Collapsed;
        RefreshAvailability();

        var entry = new CommandEntry(typed);
        _transcript.Children.Add(entry);
        _scroll.UpdateLayout();
        _scroll.ChangeView(null, _scroll.ScrollableHeight, null);

        try
        {
            // Off the UI thread: the call blocks until the host answers, and blocking the UI thread
            // on a network round trip is how an app stops repainting mid-command.
            var result = await _model.RunCommand(_host, typed);
            entry.Complete(result.Output, result.ExitCode, result.ElapsedMs);
        }
        catch (SgException error)
        {
            entry.Fail(error.Message);
        }
        finally
        {
            _running = false;
            _busy.Visibility = Visibility.Collapsed;
            _run.Visibility = Visibility.Visible;
            RefreshAvailability();
            _input.Focus(FocusState.Programmatic);

            // Follow the output the way a terminal does, rather than leaving the newest answer
            // below the fold.
            _scroll.UpdateLayout();
            _scroll.ChangeView(null, _scroll.ScrollableHeight, null);
        }
    }
}

/// <summary>One command and what came back.</summary>
internal sealed class CommandEntry : StackPanel
{
    private readonly TextBlock _status = new();
    private readonly TextBlock _output = new();

    public CommandEntry(string command)
    {
        Spacing = 5;

        var header = new Grid();
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var prompt = Widgets.Value("$", 12, Theme.Good);
        var typed = Widgets.Value(command, 12);
        typed.IsTextSelectionEnabled = true;
        typed.Margin = new Thickness(7, 0, 0, 0);
        typed.TextTrimming = TextTrimming.None;
        typed.TextWrapping = TextWrapping.Wrap;

        _status.FontFamily = Theme.Mono;
        _status.FontSize = 12;
        _status.Foreground = Theme.Brush(Theme.Tertiary);
        _status.HorizontalAlignment = HorizontalAlignment.Right;
        _status.Text = "running…";

        Grid.SetColumn(prompt, 0);
        Grid.SetColumn(typed, 1);
        Grid.SetColumn(_status, 2);
        header.Children.Add(prompt);
        header.Children.Add(typed);
        header.Children.Add(_status);
        Children.Add(header);

        _output.FontFamily = Theme.Mono;
        _output.FontSize = 11.5;
        _output.Foreground = Theme.Brush(Theme.Secondary);
        _output.IsTextSelectionEnabled = true;
        _output.TextWrapping = TextWrapping.Wrap;
        _output.Visibility = Visibility.Collapsed;
        Children.Add(_output);
    }

    public void Complete(string output, int exitCode, ulong elapsedMs)
    {
        var failed = exitCode != 0;
        _status.Text = failed ? $"exit {exitCode}" : $"{elapsedMs} ms";
        _status.Foreground = Theme.Brush(failed ? Theme.Bad : Theme.Tertiary);
        Show(output);
    }

    public void Fail(string message)
    {
        _status.Text = "failed";
        _status.Foreground = Theme.Brush(Theme.Bad);
        Show(message);
    }

    private void Show(string text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return;
        }

        _output.Text = text;
        _output.Visibility = Visibility.Visible;
    }
}

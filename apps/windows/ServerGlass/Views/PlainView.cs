using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using ServerGlass.Core;

namespace ServerGlass.Views;

/// <summary>
/// The default screen: what a person who has never heard of SSH needs to know.
/// </summary>
/// <remarks>
/// Three things, in descending order of what someone actually wants:
/// <list type="number">
/// <item><b>Is it OK?</b> One sentence, in the largest type on the screen, in a colour that answers
/// the question before the words are read.</item>
/// <item><b>Three readings</b> — processor, memory, storage — each with the quantity spelled out and
/// a trend line, because "84%" and "84%, climbing all afternoon" are different facts.</item>
/// <item><b>What is using it</b>, named.</item>
/// </list>
/// Everything else — load averages, socket counts, per-core breakdowns — is real, still collected,
/// and one click away. It is simply not what this screen is for. The wording and the choice of
/// which readings appear both come from the core, so every platform says the same thing.
/// </remarks>
internal sealed class PlainView : UserControl
{
    private readonly CoreModel _model;
    private readonly Action _showTechnical;
    private readonly StackPanel _root = new() { Spacing = 16, MaxWidth = 940 };

    /// <remarks>
    /// The model is here only for the sparkline normalisation. Every number on this screen — the
    /// tile value, the summary sentence, the health headline — arrives already worded by the core,
    /// which is the point of <c>SimpleTile</c>.
    /// </remarks>
    public PlainView(CoreModel model, Action showTechnical)
    {
        _model = model;
        _showTechnical = showTechnical;
        _root.HorizontalAlignment = HorizontalAlignment.Left;
        Content = _root;
    }

    public void Update(TargetSnapshot snapshot, string address)
    {
        _root.Children.Clear();
        _root.Children.Add(HealthCard(snapshot.Health,
            string.IsNullOrEmpty(snapshot.DisplayName) ? address : snapshot.DisplayName));

        if (snapshot.SimpleTiles.Count == 0)
        {
            _root.Children.Add(new TextBlock
            {
                Text = "Taking the first readings…",
                FontSize = 13,
                Foreground = Theme.Brush(Theme.Secondary),
                Margin = new Thickness(0, 30, 0, 30),
            });
        }
        else
        {
            // Always one row. An adaptive grid wrapped three tiles as 2 + 1 and left a hole beside
            // the last one; a fixed set of equal columns reads as a set at every width.
            var row = new Grid { ColumnSpacing = 12 };
            foreach (var _ in snapshot.SimpleTiles)
            {
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            }

            for (var i = 0; i < snapshot.SimpleTiles.Count; i++)
            {
                var tile = TileCard(snapshot.SimpleTiles[i]);
                Grid.SetColumn(tile, i);
                row.Children.Add(tile);
            }

            _root.Children.Add(row);
        }

        if (snapshot.TopProcesses.Count > 0)
        {
            _root.Children.Add(Busiest(snapshot));
        }

        _root.Children.Add(TechnicalDoor());
    }

    /// <summary>
    /// The answer to "is my server OK?", as the hero of the screen.
    /// </summary>
    /// <remarks>
    /// A tinted gradient rather than a flat wash: the card has to read as <em>the</em> answer at a
    /// glance from across a room, and a solid block of colour at this size looks like an error
    /// banner even when it is green.
    /// </remarks>
    private static Border HealthCard(HostHealth health, string name)
    {
        var tint = Theme.Level(health.Level);

        var icon = new TextBlock
        {
            Text = Theme.HealthGlyph(health.Level),
            FontFamily = Theme.Icons,
            FontSize = 30,
            Foreground = Theme.Brush(tint),
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(0, 2, 14, 0),
        };

        var text = new StackPanel { Spacing = 5 };
        text.Children.Add(new TextBlock
        {
            Text = health.Headline,
            FontSize = 22,
            FontWeight = FontWeights.SemiBold,
            FontFamily = Theme.Ui,
            Foreground = Theme.Brush(Theme.Primary),
            TextWrapping = TextWrapping.Wrap,
        });

        if (!string.IsNullOrEmpty(health.Detail))
        {
            text.Children.Add(new TextBlock
            {
                Text = health.Detail,
                FontSize = 13,
                Foreground = Theme.Brush(Theme.Secondary),
                TextWrapping = TextWrapping.Wrap,
            });
        }

        text.Children.Add(new TextBlock
        {
            Text = name,
            FontSize = 11,
            FontFamily = Theme.Mono,
            Foreground = Theme.Brush(Theme.Tertiary),
            Margin = new Thickness(0, 3, 0, 0),
        });

        var layout = new Grid();
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(icon, 0);
        Grid.SetColumn(text, 1);
        layout.Children.Add(icon);
        layout.Children.Add(text);

        var gradient = new LinearGradientBrush
        {
            StartPoint = new Windows.Foundation.Point(0, 0),
            EndPoint = new Windows.Foundation.Point(1, 1),
        };
        gradient.GradientStops.Add(new GradientStop { Color = Theme.Tint(tint, 0.16).Color, Offset = 0 });
        gradient.GradientStops.Add(new GradientStop { Color = Theme.Tint(tint, 0.05).Color, Offset = 1 });

        return new Border
        {
            Background = gradient,
            BorderBrush = Theme.Tint(tint, 0.28),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(16),
            Padding = new Thickness(16),
            Child = layout,
        };
    }

    /// <summary>
    /// One reading: a large ring, the number inside it, the quantity beneath, and a trend line.
    /// </summary>
    /// <remarks>
    /// The ring is centred and big on purpose. This is a glanceable screen, and a 42-point ring
    /// tucked beside a number reads as decoration; at this size it reads as the measurement.
    /// </remarks>
    private Border TileCard(SimpleTile tile)
    {
        var tint = Theme.Level(tile.Level);
        var stack = new StackPanel { Spacing = 11 };

        var name = Widgets.Label(tile.Name, 12.5);
        stack.Children.Add(name);

        stack.Children.Add(new RingGauge
        {
            Width = 104,
            Height = 104,
            Thickness = 8,
            Fraction = tile.Fraction ?? 0,
            RingColor = tint,
            Caption = tile.ValueText,
            CaptionSize = 21,
            HorizontalAlignment = HorizontalAlignment.Center,
        });

        // Two lines are always reserved, whether or not the text needs both. "Barely working" is
        // one line and "240.9 GiB free of 254.2 GiB" is two, and without a reservation the three
        // cards end at three different heights.
        stack.Children.Add(new TextBlock
        {
            Text = tile.Summary,
            FontSize = 11.5,
            Foreground = Theme.Brush(Theme.Tertiary),
            TextAlignment = TextAlignment.Center,
            TextWrapping = TextWrapping.Wrap,
            MaxLines = 2,
            Height = 32,
            HorizontalAlignment = HorizontalAlignment.Stretch,
        });

        // A number answers "how much"; the trend answers "is this getting worse", which is the
        // question someone glancing at a dashboard is really asking.
        if (tile.History.Count > 1)
        {
            stack.Children.Add(new Sparkline
            {
                Points = _model.Sparkline(tile.History),
                LineColor = tint,
                Height = 22,
            });
        }

        return new Border
        {
            Background = Theme.Brush(Theme.Card),
            BorderBrush = Theme.Brush(Theme.PanelBorder),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(16),
            Padding = new Thickness(14),
            Child = stack,
        };
    }

    /// <summary>
    /// Named for the question rather than the mechanism: nobody asks "what are the top processes by
    /// CPU", they ask what is making the machine busy.
    /// </summary>
    private FrameworkElement Busiest(TargetSnapshot snapshot)
    {
        var stack = new StackPanel { Spacing = 8 };
        stack.Children.Add(new TextBlock
        {
            Text = "What's keeping it busy",
            FontSize = 14,
            FontWeight = FontWeights.SemiBold,
            FontFamily = Theme.Ui,
            Foreground = Theme.Brush(Theme.Primary),
        });

        var rows = new StackPanel();
        var shown = snapshot.TopProcesses.Take(5).ToArray();
        for (var i = 0; i < shown.Length; i++)
        {
            var process = shown[i];
            var row = new Grid { Padding = new Thickness(0, 9, 0, 9) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var command = new TextBlock
            {
                Text = process.Command,
                FontSize = 13,
                Foreground = Theme.Brush(Theme.Primary),
                TextTrimming = TextTrimming.CharacterEllipsis,
            };
            Grid.SetColumn(command, 0);

            var percent = Widgets.Value(
                $"{Math.Round(process.CpuPercent):0}%", 13,
                process.CpuPercent >= 50 ? Theme.Warn : Theme.Secondary);
            Grid.SetColumn(percent, 1);

            row.Children.Add(command);
            row.Children.Add(percent);
            rows.Children.Add(row);

            if (i < shown.Length - 1)
            {
                rows.Children.Add(new Border
                {
                    Height = 1,
                    Background = Theme.Brush(Theme.PanelBorder),
                });
            }
        }

        stack.Children.Add(new Border
        {
            Background = Theme.Brush(Theme.Card),
            BorderBrush = Theme.Brush(Theme.PanelBorder),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(14),
            Padding = new Thickness(14, 2, 14, 2),
            Child = rows,
        });

        return stack;
    }

    /// <summary>
    /// The way to everything else.
    /// </summary>
    /// <remarks>
    /// It used to be grey secondary text on a grey card elsewhere, which reads as a footnote rather
    /// than a control. Tinted and captioned, it looks like the door it is.
    /// </remarks>
    private FrameworkElement TechnicalDoor()
    {
        var text = new StackPanel { Spacing = 2 };
        text.Children.Add(new TextBlock
        {
            Text = "Show every reading",
            FontSize = 13.5,
            FontWeight = FontWeights.Medium,
            FontFamily = Theme.Ui,
            Foreground = Theme.Brush(Theme.Primary),
        });
        text.Children.Add(new TextBlock
        {
            Text = "Per-core CPU, network, disks, filesystems, temperatures",
            FontSize = 11,
            Foreground = Theme.Brush(Theme.Secondary),
        });

        var layout = new Grid();
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var icon = new TextBlock
        {
            Text = "",
            FontFamily = Theme.Icons,
            FontSize = 15,
            Foreground = Theme.Brush(Theme.Info),
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 11, 0),
        };
        var chevron = new TextBlock
        {
            Text = "",
            FontFamily = Theme.Icons,
            FontSize = 11,
            Foreground = Theme.Brush(Theme.Tertiary),
            VerticalAlignment = VerticalAlignment.Center,
        };

        Grid.SetColumn(icon, 0);
        Grid.SetColumn(text, 1);
        Grid.SetColumn(chevron, 2);
        layout.Children.Add(icon);
        layout.Children.Add(text);
        layout.Children.Add(chevron);

        var button = new Button
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            Background = Theme.Brush(Theme.Card),
            BorderBrush = Theme.Tint(Theme.Info, 0.35),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(14),
            Padding = new Thickness(13),
            Content = layout,
        };
        button.Click += (_, _) => _showTechnical();
        return button;
    }
}

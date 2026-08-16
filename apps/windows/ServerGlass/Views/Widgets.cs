using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using ServerGlass.Core;
using Windows.UI;

namespace ServerGlass.Views;

/// <summary>
/// The building blocks of the dashboard, translated from <c>apps/shared/ServerGlassUI/Components.swift</c>.
/// </summary>
/// <remarks>
/// Every one of these picks its shape from the metric, not from convenience: rings only where a
/// maximum is real, bars for capacities, a monospaced number plus a sparkline for rates, and a
/// label-value row for counts. Twenty socket counters are numbers to read, not gauges to interpret.
/// </remarks>
internal static class Widgets
{
    public static TextBlock Label(string text, double size = 10) => new()
    {
        Text = text,
        FontSize = size,
        FontFamily = Theme.Ui,
        FontWeight = FontWeights.Medium,
        Foreground = Theme.Brush(Theme.Secondary),
        TextTrimming = TextTrimming.CharacterEllipsis,
    };

    /// <summary>Numbers are monospaced everywhere so a changing value does not make the layout twitch.</summary>
    // The weight struct lives in Windows.UI.Text while the named values live in Microsoft.UI.Text,
    // so it is spelled out rather than imported.
    public static TextBlock Value(string text, double size = 10.5, Color? color = null,
                                  Windows.UI.Text.FontWeight? weight = null) => new()
    {
        Text = text,
        FontSize = size,
        FontFamily = Theme.Mono,
        FontWeight = weight ?? FontWeights.Medium,
        Foreground = Theme.Brush(color ?? Theme.Primary),
        TextTrimming = TextTrimming.CharacterEllipsis,
    };

    /// <summary>
    /// A titled section. Everything on the technical page lives in one, so the page reads as a
    /// sequence of answers rather than an undifferentiated field of widgets.
    /// </summary>
    public static Border Panel(string title, string? subtitle, FrameworkElement content)
    {
        var heading = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            Margin = new Thickness(0, 0, 0, 10),
        };

        heading.Children.Add(new TextBlock
        {
            Text = title.ToUpperInvariant(),
            FontSize = 9.5,
            FontFamily = Theme.Ui,
            FontWeight = FontWeights.Medium,
            Foreground = Theme.Brush(Theme.Secondary),
        });

        if (!string.IsNullOrEmpty(subtitle))
        {
            heading.Children.Add(new TextBlock
            {
                Text = subtitle,
                FontSize = 9.5,
                FontFamily = Theme.Mono,
                Foreground = Theme.Brush(Theme.Tertiary),
                VerticalAlignment = VerticalAlignment.Bottom,
            });
        }

        var stack = new StackPanel();
        stack.Children.Add(heading);
        stack.Children.Add((FrameworkElement)content);

        return new Border
        {
            Background = Theme.Brush(Theme.Panel),
            BorderBrush = Theme.Brush(Theme.PanelBorder),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(12),
            Child = stack,
        };
    }

    /// <summary>
    /// A rate or a raw quantity: a large monospaced number with its own sparkline. No ring, because
    /// there is no maximum for it to be a fraction of.
    /// </summary>
    /// <param name="points">Already normalised by the core; see <see cref="Sparkline"/>.</param>
    public static FrameworkElement StatCell(string label, string value, Color color,
                                            IReadOnlyList<double> points)
    {
        var stack = new StackPanel { Spacing = 3 };
        stack.Children.Add(Label(label.ToUpperInvariant(), 8.5));
        stack.Children.Add(Value(value, 13, color, FontWeights.SemiBold));

        if (points.Count > 1)
        {
            stack.Children.Add(new Sparkline { Points = points, LineColor = color, Height = 14 });
        }

        return stack;
    }

    /// <summary>
    /// A label-value line. Socket counts and segment rates are numbers to be read, not gauges to be
    /// interpreted, and twenty of them belong in a compact grid.
    /// </summary>
    public static FrameworkElement KeyValueRow(string label, string value, Color? emphasis = null)
    {
        var grid = new Grid { Margin = new Thickness(0, 2, 0, 2) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var name = Label(label);
        var reading = Value(value, 10.5, emphasis);
        reading.Margin = new Thickness(8, 0, 0, 0);

        Grid.SetColumn(name, 0);
        Grid.SetColumn(reading, 1);
        grid.Children.Add(name);
        grid.Children.Add(reading);
        return grid;
    }

    /// <summary>One capacity line: name on the left, used/total on the right, bar underneath.</summary>
    public static FrameworkElement CapacityRow(string name, MetricGauge usage, MetricGauge? used,
                                        MetricGauge? total, Func<MetricGauge, string> format)
    {
        var header = new Grid();
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(52) });

        var title = Value(name, 11);
        Grid.SetColumn(title, 0);
        header.Children.Add(title);

        if (used is not null && total is not null)
        {
            var quantity = Value($"{format(used)} / {format(total)}", 10, Theme.Secondary,
                                 FontWeights.Normal);
            quantity.Margin = new Thickness(8, 0, 8, 0);
            Grid.SetColumn(quantity, 1);
            header.Children.Add(quantity);
        }

        var percent = Value(format(usage), 10, Theme.Level(usage.Severity), FontWeights.SemiBold);
        percent.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(percent, 2);
        header.Children.Add(percent);

        var stack = new StackPanel { Spacing = 5, Margin = new Thickness(0, 3, 0, 3) };
        stack.Children.Add(header);
        stack.Children.Add(new CapacityBar
        {
            Fraction = usage.Fraction ?? 0,
            BarColor = Theme.Level(usage.Severity),
        });
        return stack;
    }

    /// <summary>
    /// A headline percentage with the underlying quantity spelled out beneath it. "79.2%" alone is
    /// a number; "79.2% · 49.4 / 62.4 GiB" is an answer.
    /// </summary>
    public static FrameworkElement HeadlineRing(MetricGauge gauge, string caption, string? detail,
                                         double diameter = 76)
    {
        var stack = new StackPanel { Spacing = 7, HorizontalAlignment = HorizontalAlignment.Center };

        stack.Children.Add(new RingGauge
        {
            Width = diameter,
            Height = diameter,
            Thickness = 6,
            Fraction = gauge.Fraction ?? 0,
            RingColor = Theme.Level(gauge.Severity),
            Caption = caption,
            CaptionSize = 15,
        });

        var text = new StackPanel { Spacing = 1, HorizontalAlignment = HorizontalAlignment.Center };
        var label = Label(gauge.Label, 10);
        label.Foreground = Theme.Brush(Theme.Primary);
        label.HorizontalAlignment = HorizontalAlignment.Center;
        text.Children.Add(label);

        if (!string.IsNullOrEmpty(detail))
        {
            var sub = Value(detail, 9, Theme.Secondary, FontWeights.Normal);
            sub.HorizontalAlignment = HorizontalAlignment.Center;
            text.Children.Add(sub);
        }

        stack.Children.Add(text);
        return stack;
    }

    /// <summary>
    /// One logical CPU as a thin bar. Twenty of these read at a glance — twenty rings do not, and
    /// twenty rings is what a 20-core Proxmox host produced before this existed.
    /// </summary>
    public static FrameworkElement CoreBar(string index, MetricGauge? usage)
    {
        var percent = usage?.Value ?? 0;

        var grid = new Grid { Margin = new Thickness(0, 2, 0, 2) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(18) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(22) });

        var label = Value(index, 8.5, Theme.Tertiary, FontWeights.Normal);
        label.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(label, 0);

        var bar = new CapacityBar
        {
            Height = 5,
            Margin = new Thickness(5, 0, 5, 0),
            Fraction = percent / 100,
            BarColor = Theme.Level(usage?.Severity ?? "ok"),
            VerticalAlignment = VerticalAlignment.Center,
        };
        Grid.SetColumn(bar, 1);

        var reading = Value($"{Math.Round(percent):0}", 8.5, Theme.Secondary, FontWeights.Normal);
        reading.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(reading, 2);

        grid.Children.Add(label);
        grid.Children.Add(bar);
        grid.Children.Add(reading);
        return grid;
    }

    public static FrameworkElement Banner(string text, string detail, Color color)
    {
        var stack = new StackPanel { Spacing = 3 };

        var heading = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7 };
        heading.Children.Add(new TextBlock
        {
            Text = "",
            FontFamily = Theme.Icons,
            FontSize = 12,
            Foreground = Theme.Brush(color),
            VerticalAlignment = VerticalAlignment.Center,
        });
        heading.Children.Add(new TextBlock
        {
            Text = text,
            FontSize = 11.5,
            FontFamily = Theme.Ui,
            FontWeight = FontWeights.Medium,
            Foreground = Theme.Brush(Theme.Primary),
            TextWrapping = TextWrapping.Wrap,
        });
        stack.Children.Add(heading);

        stack.Children.Add(new TextBlock
        {
            Text = detail,
            FontSize = 10.5,
            Foreground = Theme.Brush(Theme.Secondary),
            TextWrapping = TextWrapping.Wrap,
        });

        return new Border
        {
            Background = Theme.Tint(color, 0.12),
            BorderBrush = Theme.Tint(color, 0.35),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10),
            Child = stack,
        };
    }

    /// <summary>
    /// "6 of 23" when the list is capped, "4" when it is not.
    /// </summary>
    /// <remarks>
    /// Silent truncation reads as "this is everything", which on a Proxmox host with two dozen
    /// block devices is a lie.
    /// </remarks>
    public static string Shown(int count, int cap) => count > cap ? $"{cap} of {count}" : $"{count}";

    /// <summary>A grid that wraps its children into as many columns as the width allows.</summary>
    public static Grid ColumnGrid(int columns, double spacing = 12)
    {
        var grid = new Grid { ColumnSpacing = spacing, RowSpacing = 5 };
        for (var i = 0; i < columns; i++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }

        return grid;
    }

    /// <summary>Place items across a fixed number of columns, adding rows as needed.</summary>
    public static void Fill(Grid grid, IEnumerable<FrameworkElement> items)
    {
        var columns = Math.Max(1, grid.ColumnDefinitions.Count);
        var index = 0;
        foreach (var item in items)
        {
            var row = index / columns;
            if (grid.RowDefinitions.Count <= row)
            {
                grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }

            Grid.SetRow(item, row);
            Grid.SetColumn(item, index % columns);
            grid.Children.Add(item);
            index++;
        }
    }
}

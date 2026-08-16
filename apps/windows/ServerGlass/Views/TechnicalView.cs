using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using ServerGlass.Core;

namespace ServerGlass.Views;

/// <summary>
/// One host, as a sequence of answers.
/// </summary>
/// <remarks>
/// Ordered by how people actually triage a server: is it busy, is it out of memory, is it out of
/// disk, what is the network doing, then the detail you only want when something looks wrong. Each
/// section chooses the widget that fits its metric rather than reusing one everywhere.
/// </remarks>
internal sealed class TechnicalView : UserControl
{
    /// <summary>Below this width the two-column panels stack.</summary>
    /// <remarks>
    /// Driven by measured width rather than a device class, because the case that matters is a
    /// window whose width changes while the app is running.
    /// </remarks>
    private const double WideThreshold = 680;

    /// <summary>Devices shown per panel. Anything past this is counted in the subtitle, not hidden.</summary>
    private const int DeviceCap = 6;

    private readonly CoreModel _model;
    private readonly StackPanel _root = new() { Spacing = 12 };

    public TechnicalView(CoreModel model)
    {
        _model = model;
        Content = _root;

        // Rebuild on a resize only when the two-column decision actually flips. Rebuilding on every
        // SizeChanged is a layout cycle: the new content changes the height, which fires
        // SizeChanged, which rebuilds again — and WinUI kills the process for it.
        SizeChanged += (_, _) =>
        {
            var wide = IsWide;
            if (_last is not null && wide != _wasWide)
            {
                _wasWide = wide;
                Update(_last);
            }
        };
    }

    private TargetSnapshot? _last;
    private bool _wasWide = true;

    /// <summary>Zero width means the first pass, before a parent has measured this: assume wide.</summary>
    private bool IsWide => ActualWidth >= WideThreshold || ActualWidth == 0;

    private string Format(MetricGauge gauge) => _model.Format(gauge);

    public void Update(TargetSnapshot snapshot)
    {
        _last = snapshot;
        _root.Children.Clear();

        if (snapshot.Gauges.Count == 0)
        {
            _root.Children.Add(new TextBlock
            {
                Text = "Collecting…",
                FontSize = 13,
                Foreground = Theme.Brush(Theme.Secondary),
                Margin = new Thickness(0, 40, 0, 40),
            });
            return;
        }

        var wide = IsWide;
        _wasWide = wide;

        _root.Children.Add(Overview(snapshot));
        _root.Children.Add(Pair(wide, Cpu(snapshot), Memory(snapshot)));
        _root.Children.Add(Pair(wide, Network(snapshot), Disk(snapshot)));

        if (snapshot.TopProcesses.Count > 0)
        {
            _root.Children.Add(Processes(snapshot));
        }

        var mounts = snapshot.EntitiesOfKind("fs")
            .OrderByDescending(e => e.Gauge("usage")?.Value ?? 0)
            .ToArray();
        if (mounts.Length > 0)
        {
            _root.Children.Add(Filesystems(mounts));
        }

        var sensors = snapshot.EntitiesOfKind("sensor").ToArray();
        if (sensors.Length > 0)
        {
            _root.Children.Add(Sensors(sensors));
        }

        var sockets = snapshot.Group("Sockets & TCP");
        if (sockets is not null && sockets.Gauges.Count > 0)
        {
            _root.Children.Add(Sockets(sockets));
        }

        // Anything the core grouped that has no panel of its own still gets shown, rather than
        // disappearing because this view has not been taught about it.
        foreach (var group in snapshot.DetailGroups)
        {
            if (group.Title is "Sockets & TCP" || group.Gauges.Count == 0)
            {
                continue;
            }

            if (KnownGroups.Contains(group.Title))
            {
                continue;
            }

            _root.Children.Add(OtherGroup(group));
        }
    }

    /// <summary>Groups already represented by a purpose-built panel above.</summary>
    private static readonly HashSet<string> KnownGroups =
        ["CPU", "Memory", "Network", "Disk", "Filesystems", "Sensors", "Load"];

    private static FrameworkElement Pair(bool wide, FrameworkElement first, FrameworkElement second)
    {
        if (!wide)
        {
            var stack = new StackPanel { Spacing = 12 };
            stack.Children.Add(first);
            stack.Children.Add(second);
            return stack;
        }

        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(first, 0);
        Grid.SetColumn(second, 1);
        grid.Children.Add(first);
        grid.Children.Add(second);
        return grid;
    }

    // -----------------------------------------------------------------------------------------
    // Overview — the questions asked first
    // -----------------------------------------------------------------------------------------

    private FrameworkElement Overview(TargetSnapshot snapshot)
    {
        var tiles = new List<FrameworkElement>();

        void Ring(string metric, string? detail, string? caption = null)
        {
            var gauge = snapshot.Gauge(metric);
            if (gauge is not null)
            {
                tiles.Add(Widgets.HeadlineRing(gauge, caption ?? Format(gauge), detail));
            }
        }

        string? Quantity(string used, string total)
        {
            var a = snapshot.Gauge(used);
            var b = snapshot.Gauge(total);
            return a is null || b is null ? null : $"{Format(a)} / {Format(b)}";
        }

        Ring("cpu_usage", $"{snapshot.CpuCount} cores");
        Ring("mem_usage", Quantity("mem_used", "mem_total"));
        Ring("disk_usage", "root");
        Ring("swap_usage", Quantity("swap_used", "swap_total"));
        Ring("cpu_temp", "processor");

        var load = snapshot.Gauge("load1");
        if (load is not null)
        {
            var five = snapshot.Gauge("load5");
            var fifteen = snapshot.Gauge("load15");
            var detail = five is null || fifteen is null
                ? null
                : $"{five.Value:0.00} · {fifteen.Value:0.00}";
            tiles.Add(Widgets.HeadlineRing(load, $"{load.Value:0.00}", detail));
        }

        // Uptime has no maximum, so it gets a number and no ring. Drawing one would imply a
        // fullness that does not exist.
        var uptime = snapshot.Gauge("uptime");
        if (uptime is not null)
        {
            var stack = new StackPanel { Spacing = 7, HorizontalAlignment = HorizontalAlignment.Center };
            var number = Widgets.Value(Format(uptime), 15, Theme.Primary, FontWeights.SemiBold);
            number.HorizontalAlignment = HorizontalAlignment.Center;
            number.VerticalAlignment = VerticalAlignment.Center;
            stack.Children.Add(new Border { Width = 76, Height = 76, Child = number });
            var label = Widgets.Label("Uptime", 10);
            label.Foreground = Theme.Brush(Theme.Primary);
            label.HorizontalAlignment = HorizontalAlignment.Center;
            stack.Children.Add(label);
            tiles.Add(stack);
        }

        var grid = Widgets.ColumnGrid(Math.Max(1, Math.Min(tiles.Count, 7)), 4);
        grid.RowSpacing = 12;
        Widgets.Fill(grid, tiles);
        return Widgets.Panel("Overview", null, grid);
    }

    // -----------------------------------------------------------------------------------------
    // CPU
    // -----------------------------------------------------------------------------------------

    private FrameworkElement Cpu(TargetSnapshot snapshot)
    {
        var body = new StackPanel { Spacing = 12 };

        body.Children.Add(new StackedBar
        {
            Segments =
            [
                ("User", snapshot.Gauge("cpu_user")?.Value ?? 0, Theme.Info),
                ("System", snapshot.Gauge("cpu_system")?.Value ?? 0, Theme.Warn),
                ("I/O wait", snapshot.Gauge("cpu_iowait")?.Value ?? 0, Theme.Bad),
                ("Steal", snapshot.Gauge("cpu_steal")?.Value ?? 0, Theme.Steal),
            ],
        });

        body.Children.Add(Legend(
            ("User", snapshot.Gauge("cpu_user")?.Value ?? 0, Theme.Info),
            ("System", snapshot.Gauge("cpu_system")?.Value ?? 0, Theme.Warn),
            ("I/O wait", snapshot.Gauge("cpu_iowait")?.Value ?? 0, Theme.Bad),
            ("Steal", snapshot.Gauge("cpu_steal")?.Value ?? 0, Theme.Steal)));

        var cores = snapshot.EntitiesOfKind("cpu")
            .OrderBy(e => int.TryParse(e.Display, out var n) ? n : 0)
            .ToArray();

        if (cores.Length > 0)
        {
            var grid = Widgets.ColumnGrid(cores.Length > 8 ? 2 : 1, 12);
            Widgets.Fill(grid, cores.Select(core => Widgets.CoreBar(core.Display, core.Gauge("usage"))));
            body.Children.Add(grid);
        }

        var stats = new List<FrameworkElement>();
        foreach (var metric in new[] { "procs_running", "procs_blocked", "ctx_switches" })
        {
            var gauge = snapshot.Gauge(metric);
            if (gauge is not null)
            {
                stats.Add(Widgets.StatCell(gauge.Label, Format(gauge), Theme.Primary, _model.Sparkline(gauge.History)));
            }
        }

        if (stats.Count > 0)
        {
            var grid = Widgets.ColumnGrid(stats.Count, 14);
            Widgets.Fill(grid, stats);
            body.Children.Add(grid);
        }

        return Widgets.Panel("CPU", $"{snapshot.CpuCount} logical", body);
    }

    private static FrameworkElement Legend(params (string Label, double Percent, Windows.UI.Color Color)[] segments)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        foreach (var (label, percent, color) in segments)
        {
            var item = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
            item.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse
            {
                Width = 5,
                Height = 5,
                Fill = Theme.Brush(color),
                VerticalAlignment = VerticalAlignment.Center,
            });
            item.Children.Add(Widgets.Label(label, 9));
            item.Children.Add(Widgets.Value($"{percent:0.0}%", 9));
            row.Children.Add(item);
        }

        return row;
    }

    // -----------------------------------------------------------------------------------------
    // Memory
    // -----------------------------------------------------------------------------------------

    private FrameworkElement Memory(TargetSnapshot snapshot)
    {
        var body = new StackPanel { Spacing = 12 };

        var physical = snapshot.Gauge("mem_usage");
        if (physical is not null)
        {
            body.Children.Add(Widgets.CapacityRow(
                "Physical", physical, snapshot.Gauge("mem_used"), snapshot.Gauge("mem_total"), Format));
        }

        var swap = snapshot.Gauge("swap_usage");
        if (swap is not null)
        {
            body.Children.Add(Widgets.CapacityRow(
                "Swap", swap, snapshot.Gauge("swap_used"), snapshot.Gauge("swap_total"), Format));
        }

        // The breakdown is deliberately a plain list of quantities: on a host running ZFS these do
        // not sum to the total (ARC is neither free nor counted as cached), so rendering them as a
        // stacked bar would draw a picture that is simply untrue.
        var rows = new List<FrameworkElement>();
        foreach (var metric in new[] { "mem_available", "mem_free", "mem_cached", "mem_buffers" })
        {
            var gauge = snapshot.Gauge(metric);
            if (gauge is not null)
            {
                rows.Add(Widgets.KeyValueRow(gauge.Label, Format(gauge)));
            }
        }

        if (rows.Count > 0)
        {
            var grid = Widgets.ColumnGrid(2, 12);
            Widgets.Fill(grid, rows);
            body.Children.Add(grid);
        }

        return Widgets.Panel("Memory", null, body);
    }

    // -----------------------------------------------------------------------------------------
    // Network and disk
    // -----------------------------------------------------------------------------------------

    private FrameworkElement Network(TargetSnapshot snapshot)
    {
        // Ranked by combined traffic. Sorting on receive alone buries a send-heavy uplink below
        // idle interfaces, and truncation then hides it entirely.
        var interfaces = snapshot.EntitiesOfKind("net")
            .Where(e => e.Throughput("rx_bytes", "tx_bytes") > 0)
            .OrderByDescending(e => e.Throughput("rx_bytes", "tx_bytes"))
            .ToArray();

        var body = new StackPanel { Spacing = 12 };
        body.Children.Add(RateRow(
            snapshot.Gauge("net_rx"), "Download", Theme.Good,
            snapshot.Gauge("net_tx"), "Upload", Theme.Info));

        foreach (var entity in interfaces.Take(DeviceCap))
        {
            body.Children.Add(DeviceRow(entity, "rx_bytes", "tx_bytes", Theme.Good, Theme.Info));
        }

        return Widgets.Panel("Network", Widgets.Shown(interfaces.Length, DeviceCap), body);
    }

    private FrameworkElement Disk(TargetSnapshot snapshot)
    {
        // Combined, for the same reason: reads served from ZFS ARC leave a write-heavy pool reading
        // zero, which would sort the busiest device on the box last and then cut it.
        var disks = snapshot.EntitiesOfKind("disk")
            .OrderByDescending(e => e.Throughput("read_bytes", "write_bytes"))
            .ToArray();

        var body = new StackPanel { Spacing = 12 };
        body.Children.Add(RateRow(
            snapshot.Gauge("disk_read"), "Read", Theme.Good,
            snapshot.Gauge("disk_write"), "Write", Theme.Warn));

        foreach (var entity in disks.Take(DeviceCap))
        {
            body.Children.Add(DeviceRow(entity, "read_bytes", "write_bytes", Theme.Good, Theme.Warn));
        }

        return Widgets.Panel("Disk I/O", Widgets.Shown(disks.Length, DeviceCap), body);
    }

    private FrameworkElement RateRow(MetricGauge? first, string firstLabel, Windows.UI.Color firstColor,
                              MetricGauge? second, string secondLabel, Windows.UI.Color secondColor)
    {
        var grid = Widgets.ColumnGrid(2, 14);
        var cells = new List<FrameworkElement>();

        if (first is not null)
        {
            cells.Add(Widgets.StatCell(firstLabel, Format(first), firstColor, _model.Sparkline(first.History)));
        }

        if (second is not null)
        {
            cells.Add(Widgets.StatCell(secondLabel, Format(second), secondColor, _model.Sparkline(second.History)));
        }

        Widgets.Fill(grid, cells);
        return grid;
    }

    /// <summary><c>eth0   ▁▂▃  246 KiB/s   240 KiB/s</c>, with the receive sparkline behind it.</summary>
    private FrameworkElement DeviceRow(EntityView entity, string inMetric, string outMetric,
                                Windows.UI.Color inColor, Windows.UI.Color outColor)
    {
        var grid = new Grid { ColumnSpacing = 10, Margin = new Thickness(0, 2, 0, 2) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(82) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(78) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(78) });

        var name = Widgets.Value(entity.Display, 10.5);
        Grid.SetColumn(name, 0);
        grid.Children.Add(name);

        var inbound = entity.Gauge(inMetric);
        if (inbound is not null)
        {
            var spark = new Sparkline
            {
                Points = _model.Sparkline(inbound.History),
                LineColor = inColor,
                Height = 14,
                VerticalAlignment = VerticalAlignment.Center,
            };
            Grid.SetColumn(spark, 1);
            grid.Children.Add(spark);

            var reading = Widgets.Value(Format(inbound), 10, inColor, FontWeights.Normal);
            reading.HorizontalAlignment = HorizontalAlignment.Right;
            Grid.SetColumn(reading, 2);
            grid.Children.Add(reading);
        }

        var outbound = entity.Gauge(outMetric);
        if (outbound is not null)
        {
            var reading = Widgets.Value(Format(outbound), 10, outColor, FontWeights.Normal);
            reading.HorizontalAlignment = HorizontalAlignment.Right;
            Grid.SetColumn(reading, 3);
            grid.Children.Add(reading);
        }

        return grid;
    }

    // -----------------------------------------------------------------------------------------
    // Processes
    // -----------------------------------------------------------------------------------------

    /// <summary>
    /// What is actually using the machine.
    /// </summary>
    /// <remarks>
    /// "CPU 79%" only raises a question; this is where it gets answered. A table, because a process
    /// list is something you read down, not something you gauge.
    /// </remarks>
    private FrameworkElement Processes(TargetSnapshot snapshot)
    {
        var body = new StackPanel();

        var header = ProcessGrid();
        header.Margin = new Thickness(0, 0, 0, 5);
        Add(header, 0, Widgets.Label("PID", 8.5), HorizontalAlignment.Right);
        Add(header, 1, Widgets.Label("COMMAND", 8.5), HorizontalAlignment.Left);
        Add(header, 2, Widgets.Label("CPU", 8.5), HorizontalAlignment.Right);
        Add(header, 3, Widgets.Label("MEMORY", 8.5), HorizontalAlignment.Right);
        body.Children.Add(header);

        foreach (var process in snapshot.TopProcesses)
        {
            var row = ProcessGrid();
            row.Margin = new Thickness(0, 2.5, 0, 2.5);

            Add(row, 0, Widgets.Value(process.Pid, 10, Theme.Tertiary, FontWeights.Normal),
                HorizontalAlignment.Right);

            var command = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 5 };
            command.Children.Add(Widgets.Value(process.Command, 10.5));

            // Uninterruptible sleep and zombies are worth flagging; sleeping and running are the
            // normal states and a badge for them would be pure noise.
            if (process.State is "D" or "Z")
            {
                command.Children.Add(new Border
                {
                    Background = Theme.Tint(Theme.Warn, 0.15),
                    CornerRadius = new CornerRadius(3),
                    Padding = new Thickness(3, 1, 3, 1),
                    VerticalAlignment = VerticalAlignment.Center,
                    Child = Widgets.Value(process.State, 8, Theme.Warn),
                });
            }

            Add(row, 1, command, HorizontalAlignment.Left);

            var cpu = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = 6,
                HorizontalAlignment = HorizontalAlignment.Right,
            };
            cpu.Children.Add(new CapacityBar
            {
                Width = 46,
                Height = 4,
                Fraction = process.MachineFraction,
                BarColor = Theme.Level(process.Severity),
                VerticalAlignment = VerticalAlignment.Center,
            });
            cpu.Children.Add(Widgets.Value($"{process.CpuPercent:0.0}%", 10));
            Add(row, 2, cpu, HorizontalAlignment.Right);

            // Reuse the core's byte formatter so process memory reads the same as memory everywhere
            // else on the page.
            Add(row, 3,
                Widgets.Value(_model.Format(process.MemoryBytes, "B", true), 10, Theme.Secondary,
                              FontWeights.Normal),
                HorizontalAlignment.Right);

            body.Children.Add(row);
        }

        return Widgets.Panel("Top processes", "by CPU", body);
    }

    private static Grid ProcessGrid()
    {
        var grid = new Grid { ColumnSpacing = 10 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(52) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(104) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(76) });
        return grid;
    }

    private static void Add(Grid grid, int column, FrameworkElement element, HorizontalAlignment align)
    {
        element.HorizontalAlignment = align;
        Grid.SetColumn(element, column);
        grid.Children.Add(element);
    }

    // -----------------------------------------------------------------------------------------
    // Filesystems, sensors, sockets
    // -----------------------------------------------------------------------------------------

    private FrameworkElement Filesystems(IReadOnlyList<EntityView> mounts)
    {
        var rows = mounts
            .Where(m => m.Gauge("usage") is not null)
            .Select(m => Widgets.CapacityRow(
                m.Display, m.Gauge("usage")!, m.Gauge("used"), m.Gauge("total"), Format));

        var grid = Widgets.ColumnGrid(2, 18);
        grid.RowSpacing = 10;
        Widgets.Fill(grid, rows);
        return Widgets.Panel("Filesystems", $"{mounts.Count} mounted", grid);
    }

    /// <summary>
    /// Temperatures, fans and power draw, grouped by kind rather than by chip.
    /// </summary>
    /// <remarks>
    /// Someone checking on a hot machine wants every temperature together, and the chip a reading
    /// came from is a detail below that. Hottest first, so the reading that matters is the one you
    /// see without reading the list.
    /// </remarks>
    private FrameworkElement Sensors(IReadOnlyList<EntityView> sensors)
    {
        var rows = new List<FrameworkElement>();

        foreach (var sensor in sensors
                     .Where(s => s.Gauge("temp") is not null)
                     .OrderByDescending(s => s.Gauge("temp")!.Value))
        {
            var temp = sensor.Gauge("temp")!;
            // Against the manufacturer's own limit where the chip publishes one, rather than a
            // number invented here.
            rows.Add(Widgets.KeyValueRow(sensor.Display, Format(temp), Theme.Level(temp.Severity)));
        }

        foreach (var sensor in sensors.Where(s => s.Gauge("fan") is not null))
        {
            rows.Add(Widgets.KeyValueRow(sensor.Display, Format(sensor.Gauge("fan")!)));
        }

        foreach (var sensor in sensors.Where(s => s.Gauge("power") is not null))
        {
            rows.Add(Widgets.KeyValueRow(sensor.Display, Format(sensor.Gauge("power")!)));
        }

        var grid = Widgets.ColumnGrid(3, 18);
        Widgets.Fill(grid, rows);
        return Widgets.Panel("Temperature & power", $"{sensors.Count} sensors", grid);
    }

    private FrameworkElement Sockets(DetailGroup group)
    {
        var rows = group.Gauges.Select(gauge => Widgets.KeyValueRow(
            gauge.Label,
            Format(gauge),
            gauge.Metric == "tcp_retrans" && gauge.Value > 0 ? Theme.Warn : Theme.Primary));

        var grid = Widgets.ColumnGrid(3, 18);
        Widgets.Fill(grid, rows);
        return Widgets.Panel(group.Title, null, grid);
    }

    private FrameworkElement OtherGroup(DetailGroup group)
    {
        var rows = group.Gauges.Select(gauge => Widgets.KeyValueRow(gauge.Label, Format(gauge)));
        var grid = Widgets.ColumnGrid(3, 18);
        Widgets.Fill(grid, rows);
        return Widgets.Panel(group.Title, null, grid);
    }
}

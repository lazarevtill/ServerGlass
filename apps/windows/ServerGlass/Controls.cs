using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;

// System.IO.Path arrives through implicit usings, and the ring is drawn with the XAML one.
using Path = Microsoft.UI.Xaml.Shapes.Path;

namespace ServerGlass;

/// <summary>
/// Everything here draws onto a <see cref="Canvas"/>.
/// </summary>
/// <remarks>
/// Not a stylistic choice. The first version of these controls laid their shapes out in a
/// <see cref="Grid"/> and resized them from <c>SizeChanged</c>, which is a layout cycle: the child
/// size feeds the parent's desired size, which changes the control's size, which fires
/// <c>SizeChanged</c> again. WinUI detects that and kills the process with
/// <c>LayoutCycleException</c> — the app crashed on launch before it drew a single frame. A Canvas
/// reports a desired size of zero regardless of what is inside it, so nothing a redraw does can
/// feed back into the measure pass. Each control takes its extent from the caller instead.
/// </remarks>
internal abstract class DrawnControl : UserControl
{
    protected readonly Canvas Surface = new();

    protected DrawnControl()
    {
        Content = Surface;
        // The canvas fills whatever the parent gives the control, and reports nothing back.
        Surface.HorizontalAlignment = HorizontalAlignment.Stretch;
        Surface.VerticalAlignment = VerticalAlignment.Stretch;
        IsHitTestVisible = false;
        SizeChanged += (_, _) => Redraw();
    }

    protected abstract void Redraw();

    protected void Invalidate()
    {
        if (ActualWidth > 0 && ActualHeight > 0)
        {
            Redraw();
        }
    }
}

/// <summary>
/// A ring, used <b>only</b> for metrics with a real maximum.
/// </summary>
/// <remarks>
/// A ring implies a proportion of something. Drawing one for "context switches: 26,219/s" tells the
/// reader nothing and implies a fullness that does not exist. The first build of this dashboard drew
/// a ring for every host-level series, and a 20-core Proxmox host rendered forty identical tiles.
/// </remarks>
internal sealed class RingGauge : UserControl
{
    private readonly Canvas _surface = new();
    private readonly Ellipse _track = new();
    private readonly Path _arc = new();
    private readonly TextBlock _caption = new();
    private readonly TextBlock _sub = new();

    private double _fraction;
    private Color _color = Theme.Good;

    public RingGauge()
    {
        _track.Stroke = Theme.Brush(Theme.Track);
        _arc.StrokeStartLineCap = PenLineCap.Round;
        _arc.StrokeEndLineCap = PenLineCap.Round;
        _arc.Stroke = Theme.Brush(_color);

        _surface.Children.Add(_track);
        _surface.Children.Add(_arc);

        _caption.HorizontalAlignment = HorizontalAlignment.Center;
        _caption.TextAlignment = TextAlignment.Center;
        _caption.FontFamily = Theme.Mono;
        _caption.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        _caption.Foreground = Theme.Brush(Theme.Primary);

        _sub.HorizontalAlignment = HorizontalAlignment.Center;
        _sub.TextAlignment = TextAlignment.Center;
        _sub.FontFamily = Theme.Mono;
        _sub.Foreground = Theme.Brush(Theme.Tertiary);
        _sub.Visibility = Visibility.Collapsed;

        var text = new StackPanel
        {
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            Spacing = 1,
            IsHitTestVisible = false,
        };
        text.Children.Add(_caption);
        text.Children.Add(_sub);

        // The canvas carries the ring and contributes no desired size; the text is measured
        // normally and centres itself. The control's own size comes from the caller.
        var root = new Grid();
        root.Children.Add(_surface);
        root.Children.Add(text);
        Content = root;

        SizeChanged += (_, _) => Redraw();
    }

    /// <summary>Thickness of the ring. Bigger rings carry a heavier stroke, as on the other platforms.</summary>
    public double Thickness { get; set; } = 6;

    public double Fraction
    {
        get => _fraction;
        set
        {
            _fraction = Math.Clamp(double.IsFinite(value) ? value : 0, 0, 1);
            Redraw();
        }
    }

    public Color RingColor
    {
        get => _color;
        set
        {
            _color = value;
            _arc.Stroke = Theme.Brush(value);
        }
    }

    public string Caption
    {
        get => _caption.Text;
        set => _caption.Text = value;
    }

    public string? Sub
    {
        get => _sub.Text;
        set
        {
            _sub.Text = value ?? "";
            _sub.Visibility = string.IsNullOrEmpty(value) ? Visibility.Collapsed : Visibility.Visible;
        }
    }

    public double CaptionSize
    {
        get => _caption.FontSize;
        set
        {
            _caption.FontSize = value;
            _sub.FontSize = Math.Max(8, value * 0.56);
            // Keep the number inside the ring rather than letting it run under the stroke. Width is
            // NaN until a caller sets it, and an unconstrained caption is better than a NaN one.
            _caption.MaxWidth = double.IsFinite(Width)
                ? Math.Max(24, Width - (Thickness * 2) - 12)
                : double.PositiveInfinity;
        }
    }

    private void Redraw()
    {
        var size = Math.Min(ActualWidth, ActualHeight);
        if (size <= 0)
        {
            return;
        }

        var diameter = size - Thickness;
        var radius = diameter / 2;
        var centre = new Point(ActualWidth / 2, ActualHeight / 2);

        _track.StrokeThickness = Thickness;
        _track.Width = diameter;
        _track.Height = diameter;
        Canvas.SetLeft(_track, centre.X - radius);
        Canvas.SetTop(_track, centre.Y - radius);

        _arc.StrokeThickness = Thickness;

        if (_fraction <= 0.0005)
        {
            _arc.Data = null;
            return;
        }

        // A full sweep cannot be expressed as one arc segment: at 360° the start and end points
        // coincide, which draws nothing at all rather than a complete ring.
        if (_fraction >= 0.9995)
        {
            _arc.Data = new EllipseGeometry { Center = centre, RadiusX = radius, RadiusY = radius };
            return;
        }

        var sweep = _fraction * 2 * Math.PI;
        var figure = new PathFigure
        {
            StartPoint = new Point(centre.X, centre.Y - radius),
            IsClosed = false,
        };
        figure.Segments.Add(new ArcSegment
        {
            Point = new Point(
                centre.X + (radius * Math.Sin(sweep)),
                centre.Y - (radius * Math.Cos(sweep))),
            Size = new Size(radius, radius),
            IsLargeArc = _fraction > 0.5,
            SweepDirection = SweepDirection.Clockwise,
        });

        var geometry = new PathGeometry();
        geometry.Figures.Add(figure);
        _arc.Data = geometry;
    }
}

/// <summary>
/// A trend line.
/// </summary>
/// <remarks>
/// <para>
/// This draws points; it does not decide them. <see cref="Points"/> are already normalised to 0-1
/// by <c>ServerGlassCore.SparklinePoints</c>, which is the core's <c>sparkline_points</c>. The rule
/// it applies — scale to the observed range, but floor the span at a fraction of the magnitude so
/// storage creeping from 5.19% to 5.20% draws level instead of as a cliff — is a claim about what
/// the chart means, and it lives in Rust for the same reason the thresholds do.
/// </para>
/// <para>
/// It was written by hand in Swift and again in Kotlin before it moved into the core. This is the
/// front-end that does not write it a third time.
/// </para>
/// </remarks>
internal sealed class Sparkline : DrawnControl
{
    private readonly Polygon _fill = new();
    private readonly Polyline _line = new();
    private IReadOnlyList<double> _points = [];
    private Color _lineColor = Theme.Good;

    public Sparkline()
    {
        _line.StrokeThickness = 1.3;
        _line.StrokeLineJoin = PenLineJoin.Round;
        _line.Stroke = Theme.Tint(_lineColor, 0.9);
        _fill.Fill = Theme.Tint(_lineColor, 0.14);

        Surface.Children.Add(_fill);
        Surface.Children.Add(_line);
    }

    /// <summary>Already normalised to 0-1 by the core, oldest first.</summary>
    public IReadOnlyList<double> Points
    {
        get => _points;
        set
        {
            _points = value;
            Invalidate();
        }
    }

    public Color LineColor
    {
        get => _lineColor;
        set
        {
            _lineColor = value;
            _line.Stroke = Theme.Tint(value, 0.9);
            // A faint fill under the line gives the eye a shape to read at a glance, which a 1px
            // stroke at this size does not.
            _fill.Fill = Theme.Tint(value, 0.14);
        }
    }

    protected override void Redraw()
    {
        var width = ActualWidth;
        var height = ActualHeight;
        if (width <= 0 || height <= 0 || _points.Count < 2)
        {
            _line.Points.Clear();
            _fill.Points.Clear();
            return;
        }

        var points = new PointCollection();
        for (var i = 0; i < _points.Count; i++)
        {
            var x = width * i / Math.Max(_points.Count - 1, 1);
            // The only arithmetic here is turning a 0-1 position into a y coordinate. Everything
            // that decides *what* that position means happened in the core.
            points.Add(new Point(x, height - (Math.Clamp(_points[i], 0, 1) * height)));
        }

        _line.Points = points;

        var filled = new PointCollection { new(points[0].X, height) };
        foreach (var point in points)
        {
            filled.Add(point);
        }

        filled.Add(new Point(points[^1].X, height));
        _fill.Points = filled;
    }
}

/// <summary>
/// Used-of-total, as a bar.
/// </summary>
/// <remarks>
/// Filesystems and memory are capacities, not proportions of an abstract whole, and a bar shows how
/// much room is left far better than a ring does.
/// </remarks>
internal sealed class CapacityBar : DrawnControl
{
    private readonly Rectangle _track = new();
    private readonly Rectangle _fill = new();
    private double _fraction;
    private Color _barColor = Theme.Good;

    public CapacityBar()
    {
        _track.Fill = Theme.Brush(Theme.Track);
        _fill.Fill = Theme.Brush(_barColor);
        Surface.Children.Add(_track);
        Surface.Children.Add(_fill);
        Height = 6;
    }

    public double Fraction
    {
        get => _fraction;
        set
        {
            _fraction = Math.Clamp(double.IsFinite(value) ? value : 0, 0, 1);
            Invalidate();
        }
    }

    public Color BarColor
    {
        get => _barColor;
        set
        {
            _barColor = value;
            _fill.Fill = Theme.Brush(value);
        }
    }

    protected override void Redraw()
    {
        var width = ActualWidth;
        var height = ActualHeight;
        if (width <= 0 || height <= 0)
        {
            return;
        }

        var radius = height / 2;

        _track.Width = width;
        _track.Height = height;
        _track.RadiusX = radius;
        _track.RadiusY = radius;

        // A floor of two pixels: a non-zero reading that rounds away looks identical to no reading.
        _fill.Width = _fraction <= 0 ? 0 : Math.Max(2, width * _fraction);
        _fill.Height = height;
        _fill.RadiusX = radius;
        _fill.RadiusY = radius;
    }
}

/// <summary>A stacked breakdown bar — user / system / iowait / steal as proportions of one whole.</summary>
internal sealed class StackedBar : DrawnControl
{
    private IReadOnlyList<(string Label, double Percent, Color Color)> _segments = [];

    public StackedBar() => Height = 8;

    public IReadOnlyList<(string Label, double Percent, Color Color)> Segments
    {
        get => _segments;
        set
        {
            _segments = value;
            Invalidate();
        }
    }

    protected override void Redraw()
    {
        Surface.Children.Clear();

        var width = ActualWidth;
        var height = ActualHeight;
        if (width <= 0 || height <= 0)
        {
            return;
        }

        var radius = height / 2;
        var x = 0.0;

        foreach (var segment in _segments)
        {
            var percent = Math.Clamp(double.IsFinite(segment.Percent) ? segment.Percent : 0, 0, 100);
            var span = width * percent / 100;
            if (span <= 0)
            {
                continue;
            }

            var block = new Rectangle
            {
                Width = span,
                Height = height,
                Fill = Theme.Brush(segment.Color),
                RadiusX = radius,
                RadiusY = radius,
            };
            Canvas.SetLeft(block, x);
            Surface.Children.Add(block);
            x += span;
        }

        // The remainder is idle. Drawn as track rather than left empty, so the bar always reads as
        // a whole rather than as a partial measurement.
        if (x < width)
        {
            var rest = new Rectangle
            {
                Width = width - x,
                Height = height,
                Fill = Theme.Brush(Theme.Track),
                RadiusX = radius,
                RadiusY = radius,
            };
            Canvas.SetLeft(rest, x);
            Surface.Children.Add(rest);
        }
    }
}

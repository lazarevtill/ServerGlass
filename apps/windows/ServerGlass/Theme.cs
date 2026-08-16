using Microsoft.UI;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace ServerGlass;

/// <summary>
/// The visual language, translated value for value from <c>apps/shared/ServerGlassUI/DesignSystem.swift</c>.
/// </summary>
/// <remarks>
/// <para>
/// The guiding rule is that <b>the widget must match the metric</b>. A ring implies a proportion of
/// something; drawing one for "context switches per second" tells the reader nothing and, worse,
/// implies a fullness that does not exist. So:
/// </para>
/// <list type="table">
/// <item><term>Percentage (has a maximum)</term><description>ring gauge</description></item>
/// <item><term>Capacity (used of total)</term><description>horizontal bar with used / total</description></item>
/// <item><term>Rate (bytes/s, ops/s)</term><description>large monospaced number + sparkline</description></item>
/// <item><term>Count / state</term><description>plain label-value row</description></item>
/// </list>
/// <para>
/// The colours are the same numbers the Apple and Android apps use, on purpose: someone moving
/// between their phone and their desk should not have to relearn the dashboard.
/// </para>
/// </remarks>
internal static class Theme
{
    private static Color Rgb(byte r, byte g, byte b) => Color.FromArgb(255, r, g, b);

    private static Color White(double opacity) =>
        Color.FromArgb((byte)Math.Round(opacity * 255), 255, 255, 255);

    // Surfaces. `Card` is lifted from `Panel` because a single flat surface colour across every
    // size makes the plain view's big cards look like empty space.
    public static readonly Color Background = Rgb(11, 11, 13);
    public static readonly Color Panel = Rgb(21, 21, 24);
    public static readonly Color Card = Rgb(25, 25, 29);
    public static readonly Color PanelBorder = White(0.06);
    public static readonly Color Track = White(0.08);

    // Text.
    public static readonly Color Primary = White(0.92);
    public static readonly Color Secondary = White(0.45);
    public static readonly Color Tertiary = White(0.28);

    // Levels.
    public static readonly Color Good = Rgb(89, 214, 140);
    public static readonly Color Warn = Rgb(250, 191, 71);
    public static readonly Color Bad = Rgb(247, 112, 112);
    public static readonly Color Info = Rgb(107, 168, 247);
    public static readonly Color Steal = Rgb(167, 139, 250);

    /// <summary>
    /// The colour for a level the core assigned — to a host's health, a reading, or a process.
    /// </summary>
    /// <remarks>
    /// This mapping is the whole of what a view layer decides. The thresholds behind these levels
    /// used to live in each front-end and had already drifted apart: Android coloured at 0.75/0.90
    /// where Apple used 0.60/0.85, so the same host read amber on a phone and green on a desk for
    /// days. Deciding what counts as "busy" is the core's job.
    /// </remarks>
    public static Color Level(string level) => level switch
    {
        "ok" => Good,
        "busy" => Warn,
        "problem" or "offline" => Bad,
        "none" => Info,
        _ => Secondary,
    };

    public static SolidColorBrush Brush(Color color) => new(color);

    /// <summary>
    /// Put a dialog in the app's palette.
    /// </summary>
    /// <remarks>
    /// Without this a dialog's primary button is painted in the user's Windows accent colour, which
    /// on this machine is orange — and an orange "Add" on a dashboard whose entire vocabulary is
    /// green/amber/red reads as a warning. The accent here is the same blue the app uses for
    /// "neutral, no threshold crossed".
    /// </remarks>
    public static void StyleDialog(Microsoft.UI.Xaml.Controls.ContentDialog dialog)
    {
        dialog.RequestedTheme = Microsoft.UI.Xaml.ElementTheme.Dark;
        dialog.Background = Brush(Panel);
        dialog.BorderBrush = Brush(PanelBorder);
        dialog.Foreground = Brush(Primary);

        dialog.Resources["AccentButtonBackground"] = Brush(Info);
        dialog.Resources["AccentButtonBackgroundPointerOver"] = Tint(Info, 0.85);
        dialog.Resources["AccentButtonBackgroundPressed"] = Tint(Info, 0.70);
        dialog.Resources["AccentButtonForeground"] = Brush(Background);
        dialog.Resources["AccentButtonForegroundPointerOver"] = Brush(Background);
        dialog.Resources["AccentButtonForegroundPressed"] = Brush(Background);
        dialog.Resources["ContentDialogBackground"] = Brush(Panel);
        dialog.Resources["ContentDialogTopOverlay"] = Brush(Panel);
    }

    public static SolidColorBrush LevelBrush(string level) => new(Level(level));

    /// <summary>A level tinted for use as a fill behind text.</summary>
    public static SolidColorBrush Tint(Color color, double opacity) =>
        new(Color.FromArgb((byte)Math.Round(opacity * 255), color.R, color.G, color.B));

    /// <summary>
    /// The glyph for a health level, in one place so "problem" always looks like a problem.
    /// Segoe Fluent Icons, which ships with Windows 11 and is present on Windows 10 as Segoe MDL2.
    /// </summary>
    public static string HealthGlyph(string level) => level switch
    {
        "ok" => "",       // Completed
        "busy" => "",     // Warning
        "problem" => "",  // Error
        "offline" => "",  // NetworkOffline
        _ => "",          // Sync / checking
    };

    // Numbers are monospaced everywhere so columns line up and a changing value does not make the
    // layout twitch on every refresh. Cascadia Mono ships with Windows Terminal and Windows 11;
    // Consolas is the floor on any Windows since Vista.
    public static readonly FontFamily Mono = new("Cascadia Mono, Consolas, Courier New");
    public static readonly FontFamily Ui = new("Segoe UI Variable Display, Segoe UI");
    public static readonly FontFamily Icons = new("Segoe Fluent Icons, Segoe MDL2 Assets");
}

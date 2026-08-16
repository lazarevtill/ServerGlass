using Microsoft.UI.Xaml;

namespace ServerGlass;

public partial class App : Application
{
    private Window? _window;

    public App()
    {
        InitializeComponent();

        // A WinUI crash surfaces to the user as the window vanishing and to the event log as
        // "stowed exception" with no stack. Writing the exception somewhere readable is the
        // difference between a diagnosable failure and a silent one.
        UnhandledException += (_, e) => Record(e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => Record(e.ExceptionObject as Exception);
        TaskScheduler.UnobservedTaskException += (_, e) => Record(e.Exception);
    }

    /// <summary>Where a crash is written. Follows the same override the rest of the app honours.</summary>
    internal static string LogPath => Path.Combine(
        Environment.GetEnvironmentVariable(Core.HostStore.DirectoryVariable)
        ?? Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "ServerGlass"),
        "crash.log");

    private static void Record(Exception? error)
    {
        if (error is null)
        {
            return;
        }

        try
        {
            var path = LogPath;
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.AppendAllText(path, $"{DateTimeOffset.Now:O}\n{error}\n\n");
        }
        catch (Exception)
        {
            // Logging a crash must never itself crash the process.
        }
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            _window = new MainWindow();
            _window.Activate();
        }
        catch (Exception error)
        {
            Record(error);
            throw;
        }
    }
}

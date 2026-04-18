using System.Diagnostics;

namespace CloseMinesGame;

/// <summary>
/// Завершает процесс игры (не установщик). Имя = колонка «Имя» в диспетчере задач, без .exe.
/// </summary>
internal static class Program
{
    private const string GameProcessName = "MinesUnityProject";

    private const int KillTimeoutMs = 1500;

    private static int Main()
    {
        foreach (var process in Process.GetProcessesByName(GameProcessName))
        {
            try
            {
                if (process.MainWindowHandle != IntPtr.Zero)
                {
                    process.CloseMainWindow();
                    if (process.WaitForExit(KillTimeoutMs))
                    {
                        continue;
                    }
                }

                process.Kill();
            }
            catch
            {
                try
                {
                    process.Kill();
                }
                catch
                {
                }
            }
            finally
            {
                process.Dispose();
            }
        }

        return 0;
    }
}

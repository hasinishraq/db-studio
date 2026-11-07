# Answers one native file dialog of the given process, through its window handles. It sends no
# keystrokes, so it cannot type into any other window.
param([int]$ProcessId, [string]$Path, [int]$Seconds = 15)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class FileDialog {
  delegate bool Visit(IntPtr handle, IntPtr extra);
  [DllImport("user32")] static extern bool EnumWindows(Visit visit, IntPtr extra);
  [DllImport("user32")] static extern bool EnumChildWindows(IntPtr parent, Visit visit, IntPtr extra);
  [DllImport("user32")] static extern uint GetWindowThreadProcessId(IntPtr handle, out uint process);
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr handle, StringBuilder name, int size);
  [DllImport("user32")] static extern bool IsWindowVisible(IntPtr handle);
  [DllImport("user32")] static extern int GetDlgCtrlID(IntPtr handle);
  [DllImport("user32")] static extern IntPtr GetParent(IntPtr handle);
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr handle, uint message, IntPtr wide, string text);
  [DllImport("user32")] static extern bool PostMessage(IntPtr handle, uint message, IntPtr wide, IntPtr extra);

  const uint SET_TEXT = 0x000C;
  const uint CLICK = 0x00F5;
  const int OK_BUTTON = 1;

  static string ClassOf(IntPtr handle) {
    var name = new StringBuilder(64);
    GetClassName(handle, name, 64);
    return name.ToString();
  }

  public static StringBuilder Seen = new StringBuilder();

  public static bool Answer(int process, string path) {
    Seen.Clear();
    IntPtr dialog = IntPtr.Zero;
    EnumWindows((handle, _) => {
      uint owner;
      GetWindowThreadProcessId(handle, out owner);
      if (owner == process && IsWindowVisible(handle) && ClassOf(handle) == "#32770") {
        dialog = handle;
        return false;
      }
      return true;
    }, IntPtr.Zero);
    if (dialog == IntPtr.Zero) return false;

    IntPtr edit = IntPtr.Zero;
    IntPtr ok = IntPtr.Zero;
    EnumChildWindows(dialog, (handle, _) => {
      string name = ClassOf(handle);
      Seen.AppendLine(name + " id=" + GetDlgCtrlID(handle) + " visible=" + IsWindowVisible(handle));
      // The address bar has an Edit too, but it stays hidden until clicked.
      if (name == "Edit" && edit == IntPtr.Zero && IsWindowVisible(handle) && ClassOf(GetParent(handle)) == "ComboBox") {
        edit = handle;
      }
      if (name == "Button" && GetDlgCtrlID(handle) == OK_BUTTON) ok = handle;
      return true;
    }, IntPtr.Zero);
    if (edit == IntPtr.Zero || ok == IntPtr.Zero) return false;

    SendMessage(edit, SET_TEXT, IntPtr.Zero, path);
    PostMessage(ok, CLICK, IntPtr.Zero, IntPtr.Zero);
    return true;
  }
}
'@

$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
  if ([FileDialog]::Answer($ProcessId, $Path)) { 'answered'; exit 0 }
  Start-Sleep -Milliseconds 200
}
'no dialog appeared, or it had no file name box'
[FileDialog]::Seen.ToString()
exit 1

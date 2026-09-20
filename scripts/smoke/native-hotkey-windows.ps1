param(
    [Parameter(Mandatory = $true)]
    [string]$Binary
)

$ErrorActionPreference = "Stop"
$Expected = "JamePrompt native smoke ñ 123"
$ResolvedBinary = (Resolve-Path $Binary).Path
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("jameprompt-hotkey-smoke-" + [Guid]::NewGuid())
$StdoutLog = Join-Path $TempDir "jame-prompt.stdout.log"
$StderrLog = Join-Path $TempDir "jame-prompt.stderr.log"
$App = $null
$script:Success = $false
$script:Failure = $null

New-Item -ItemType Directory -Path $TempDir | Out-Null
$env:JAME_PROMPT_UI_SMOKE_DURATION_MS = "30000"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class JamePromptNativeHotkeySmokeInput
{
    private const uint INPUT_KEYBOARD = 1;
    private const uint KEYEVENTF_KEYUP = 0x0002;
    private const ushort VK_CONTROL = 0x11;
    private const ushort VK_SHIFT = 0x10;
    private const ushort VK_P = 0x50;

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT
    {
        public uint type;
        public InputUnion U;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct InputUnion
    {
        [FieldOffset(0)]
        public KEYBDINPUT ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct KEYBDINPUT
    {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    private static INPUT Key(ushort virtualKey, uint flags)
    {
        INPUT input = new INPUT();
        input.type = INPUT_KEYBOARD;
        input.U.ki.wVk = virtualKey;
        input.U.ki.dwFlags = flags;
        return input;
    }

    public static void SendCtrlShiftP()
    {
        INPUT[] inputs = new INPUT[]
        {
            Key(VK_CONTROL, 0),
            Key(VK_SHIFT, 0),
            Key(VK_P, 0),
            Key(VK_P, KEYEVENTF_KEYUP),
            Key(VK_SHIFT, KEYEVENTF_KEYUP),
            Key(VK_CONTROL, KEYEVENTF_KEYUP),
        };

        uint sent = SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT)));
        if (sent != inputs.Length)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SendInput did not inject the complete smoke hotkey");
        }
    }
}
"@

try {
    $App = Start-Process -FilePath $ResolvedBinary -ArgumentList "--native-hotkey-smoke" -RedirectStandardOutput $StdoutLog -RedirectStandardError $StderrLog -PassThru

    Start-Sleep -Milliseconds 1500

    if ($App.HasExited) {
        throw "JamePrompt exited before the smoke target was created. Exit code: $($App.ExitCode)"
    }

    $Form = New-Object System.Windows.Forms.Form
    $Form.Text = "JamePrompt Native Hotkey Smoke Target"
    $Form.Width = 700
    $Form.Height = 180
    $Form.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterScreen
    $Form.TopMost = $true

    $TextBox = New-Object System.Windows.Forms.TextBox
    $TextBox.Dock = [System.Windows.Forms.DockStyle]::Fill
    $TextBox.Multiline = $true
    $TextBox.Font = New-Object System.Drawing.Font("Segoe UI", 12)
    $Form.Controls.Add($TextBox)

    $Trigger = New-Object System.Windows.Forms.Timer
    $Trigger.Interval = 1500
    $Trigger.Add_Tick({
        $Trigger.Stop()

        if ($App.HasExited) {
            $script:Failure = "JamePrompt exited before the global hotkey was injected"
            $Form.Close()
            return
        }

        $Form.Activate()
        $TextBox.Focus() | Out-Null
        [System.Windows.Forms.Application]::DoEvents()
        [JamePromptNativeHotkeySmokeInput]::SetForegroundWindow($Form.Handle) | Out-Null
        Start-Sleep -Milliseconds 100

        if ([JamePromptNativeHotkeySmokeInput]::GetForegroundWindow() -ne $Form.Handle) {
            $script:Failure = "WinForms target could not become the foreground window"
            $Form.Close()
            return
        }

        try {
            [JamePromptNativeHotkeySmokeInput]::SendCtrlShiftP()
        }
        catch {
            $script:Failure = "Failed to inject the smoke hotkey with SendInput: $($_.Exception.Message)"
            $Form.Close()
        }
    })

    $Timeout = New-Object System.Windows.Forms.Timer
    $Timeout.Interval = 10000
    $Timeout.Add_Tick({
        $Timeout.Stop()
        $script:Failure = "Timed out waiting for JamePrompt to paste into the WinForms target. Actual text: '$($TextBox.Text)'"
        $Form.Close()
    })

    $TextBox.Add_TextChanged({
        if ($TextBox.Text -eq $Expected) {
            $script:Success = $true
            $Form.Close()
        }
        elseif ($TextBox.Text.Length -gt $Expected.Length) {
            $script:Failure = "WinForms target received unexpected text: '$($TextBox.Text)'"
            $Form.Close()
        }
    })

    $Form.Add_Shown({
        $TextBox.Focus() | Out-Null
        $Trigger.Start()
        $Timeout.Start()
    })

    [System.Windows.Forms.Application]::Run($Form)

    $Trigger.Stop()
    $Timeout.Stop()
    $Trigger.Dispose()
    $Timeout.Dispose()
    $TextBox.Dispose()
    $Form.Dispose()

    if (-not $script:Success) {
        if ($script:Failure) {
            throw $script:Failure
        }
        throw "Native hotkey smoke ended without receiving the expected text"
    }

    Write-Host "Windows native hotkey auto-paste smoke passed"
}
finally {
    if ($App -and -not $App.HasExited) {
        Stop-Process -Id $App.Id -Force -ErrorAction SilentlyContinue
        $App.WaitForExit()
    }

    if (-not $script:Success) {
        if (Test-Path $StdoutLog) {
            Write-Host "----- JamePrompt stdout -----"
            Get-Content $StdoutLog
        }
        if (Test-Path $StderrLog) {
            Write-Host "----- JamePrompt stderr -----"
            Get-Content $StderrLog
        }
    }

    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}

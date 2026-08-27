# Make the robot speak. PowerShell-only: Python on this operator PC is dead.
#
#   $env:HOMBOT_HOST = '...'
#   $env:HOMBOT_SERVE_IP = '...'   # this PC, as the robot can reach it
#   .\speak.ps1 "Hallo Storax"
#
# Transfer is HTTP + wget (Dropbear has no sftp; ssh stdin is unusable).
# Playback is unmute-then-aplay: LG /Playback holds the only WM8960 subdevice.
# aplay on a "busy" card still works once Speaker Playback Off is Stereo.
# Skipping the busy card (say.py) leaves no device at all on this robot.
# BusyBox wget -O will not overwrite: rm the dest first.

param(
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Text,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$phrase = ($Text -join ' ').Trim()
if (-not $phrase) { throw 'usage: speak.ps1 "text"' }

function Require-Env([string]$Name) {
    $v = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($v)) {
        throw "$Name is not set. This repo ships no LAN addresses -- see docs/OPERATOR_TOOLS.md."
    }
    return $v
}

$HostName = Require-Env 'HOMBOT_HOST'
$ServeIp = Require-Env 'HOMBOT_SERVE_IP'
$ServePort = if ($env:HOMBOT_SERVE_PORT) { [int]$env:HOMBOT_SERVE_PORT } else { 8099 }
$User = if ($env:HOMBOT_USER) { $env:HOMBOT_USER } else { 'root' }
$Voice = if ($env:HOMBOT_VOICE) { $env:HOMBOT_VOICE } else { 'Microsoft Hedda Desktop' }
$Rate = 16000
$LeadMs = 200

$secretFile = Join-Path $PSScriptRoot '.hombot_secret'
if ([string]::IsNullOrWhiteSpace($env:HOMBOT_LOGIN_SECRET) -and (Test-Path $secretFile)) {
    $env:HOMBOT_LOGIN_SECRET = (Get-Content -LiteralPath $secretFile -Raw).Trim()
}

$askpass = $env:HOMBOT_ASKPASS
if ([string]::IsNullOrWhiteSpace($askpass)) {
    $askpass = Join-Path $env:USERPROFILE '.config\hombot\askpass.cmd'
}
$knownHosts = Join-Path $env:USERPROFILE '.config\hombot\known_hosts'
New-Item -ItemType Directory -Force -Path (Split-Path $knownHosts) | Out-Null

$work = Join-Path $env:TEMP ("hombot_speak_" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $work | Out-Null

$env:DISPLAY = '127.0.0.1:0'
if (Test-Path -LiteralPath $askpass) {
    $env:SSH_ASKPASS = $askpass
    $env:SSH_ASKPASS_REQUIRE = 'force'
} elseif (-not [string]::IsNullOrWhiteSpace($env:HOMBOT_LOGIN_SECRET)) {
    $tmpAsk = Join-Path $work 'askpass.cmd'
    # Echo the env var. Do not write the password into the file.
    Set-Content -LiteralPath $tmpAsk -Value "@echo off`r`necho %HOMBOT_LOGIN_SECRET%" -Encoding Ascii
    $env:SSH_ASKPASS = $tmpAsk
    $env:SSH_ASKPASS_REQUIRE = 'force'
} else {
    throw 'No SSH askpass and HOMBOT_LOGIN_SECRET is not set. See docs/OPERATOR_TOOLS.md.'
}

$sshArgs = @(
    '-o', 'BatchMode=no',
    '-o', 'StrictHostKeyChecking=accept-new',
    '-o', "UserKnownHostsFile=$knownHosts",
    '-o', 'PreferredAuthentications=password',
    '-o', 'PubkeyAuthentication=no',
    '-o', 'ConnectTimeout=8',
    '-o', 'KexAlgorithms=+diffie-hellman-group1-sha1',
    '-o', 'HostKeyAlgorithms=+ssh-rsa',
    '-o', 'PubkeyAcceptedAlgorithms=+ssh-rsa',
    '-o', 'MACs=+hmac-sha1',
    '-c', 'aes128-cbc,3des-cbc,aes128-ctr',
    "${User}@${HostName}"
)

function Invoke-Robot {
    param([string]$Command, [int]$Attempts = 8)
    $last = $null
    for ($i = 0; $i -lt $Attempts; $i++) {
        $out = & ssh @sshArgs $Command 2>&1
        $code = $LASTEXITCODE
        $text = ($out | Out-String).Trim()
        if ($code -eq 0) { return $text }
        $last = $text
        Start-Sleep -Seconds 3
    }
    throw "robot unreachable after $Attempts attempts: $last"
}

function Get-PcmFromWav([string]$Path) {
    $fs = [IO.File]::OpenRead($Path)
    $br = New-Object IO.BinaryReader $fs
    try {
        $riff = [Text.Encoding]::ASCII.GetString($br.ReadBytes(4))
        if ($riff -ne 'RIFF') { throw "Hedda did not write a RIFF wav" }
        [void]$br.ReadUInt32()
        $wave = [Text.Encoding]::ASCII.GetString($br.ReadBytes(4))
        if ($wave -ne 'WAVE') { throw "Hedda did not write a WAVE file" }
        $data = $null
        $channels = 0
        $rateHz = 0
        $bits = 0
        while ($fs.Position -le ($fs.Length - 8)) {
            $id = [Text.Encoding]::ASCII.GetString($br.ReadBytes(4))
            $len = $br.ReadUInt32()
            $next = $fs.Position + $len
            if ($id -eq 'fmt ') {
                [void]$br.ReadUInt16()
                $channels = $br.ReadUInt16()
                $rateHz = [int]$br.ReadUInt32()
                [void]$br.ReadUInt32()
                [void]$br.ReadUInt16()
                $bits = $br.ReadUInt16()
            } elseif ($id -eq 'data') {
                $data = $br.ReadBytes([int]$len)
            }
            if ($next % 2 -eq 1) { $next++ }
            $fs.Position = [Math]::Min($next, $fs.Length)
        }
        if ($null -eq $data -or $data.Length -eq 0) { throw 'Hedda wrote an empty wav' }
        return @{ Bytes = $data; Channels = $channels; Rate = $rateHz; Bits = $bits }
    } finally {
        $br.Close()
    }
}

$wav = Join-Path $work 'say.wav'
$snd = Join-Path $work 'say.snd'
$listener = $null
$runspace = $null
$pshell = $null

try {
    Add-Type -AssemblyName System.Speech
    $fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
        $Rate,
        [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
        [System.Speech.AudioFormat.AudioChannel]::Mono
    )
    $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
    try {
        $synth.SelectVoice($Voice)
        $synth.Volume = 100
        $synth.SetOutputToWaveFile($wav, $fmt)
        $synth.Speak($phrase)
    } finally {
        $synth.Dispose()
    }

    $pcm = Get-PcmFromWav $wav
    if ($pcm.Rate -ne $Rate -or $pcm.Channels -ne 1 -or $pcm.Bits -ne 16) {
        throw "Hedda wav was $($pcm.Rate) Hz $($pcm.Channels)ch $($pcm.Bits)-bit; need ${Rate} Hz mono s16"
    }
    $pad = New-Object byte[] ($Rate * 2 * $LeadMs / 1000)
    $payload = New-Object byte[] ($pad.Length + $pcm.Bytes.Length)
    [Array]::Copy($pad, 0, $payload, 0, $pad.Length)
    [Array]::Copy($pcm.Bytes, 0, $payload, $pad.Length, $pcm.Bytes.Length)
    [IO.File]::WriteAllBytes($snd, $payload)
    $size = $payload.Length

    $prefix = "http://${ServeIp}:${ServePort}/"
    $runspace = [runspacefactory]::CreateRunspace()
    $runspace.Open()
    $pshell = [powershell]::Create()
    $pshell.Runspace = $runspace
    [void]$pshell.AddScript({
        param($prefix, $bytes)
        $l = New-Object System.Net.HttpListener
        $l.Prefixes.Add($prefix)
        $l.Start()
        try {
            $iar = $l.BeginGetContext($null, $null)
            if (-not $iar.AsyncWaitHandle.WaitOne(30000)) {
                throw 'robot did not fetch the clip within 30s'
            }
            $ctx = $l.EndGetContext($iar)
            $ctx.Response.StatusCode = 200
            $ctx.Response.ContentType = 'application/octet-stream'
            $ctx.Response.ContentLength64 = $bytes.Length
            $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
            $ctx.Response.Close()
        } finally {
            $l.Stop()
            $l.Close()
        }
    }).AddArgument($prefix).AddArgument($payload)
    $async = $pshell.BeginInvoke()
    Start-Sleep -Milliseconds 400

    $fetched = Invoke-Robot (
        "rm -f /tmp/say.snd; " +
        "wget -q -O /tmp/say.snd http://${ServeIp}:${ServePort}/say.snd 2>&1; " +
        "wc -c < /tmp/say.snd"
    )
    if ($fetched -notmatch '^\d+$' -or [int]$fetched -ne $size) {
        throw "robot fetched '$fetched' of $size bytes -- is HOMBOT_SERVE_IP ($ServeIp) the address it can reach this machine on?"
    }

    try { $pshell.EndInvoke($async) | Out-Null } catch { }

    # Unmute then play. Do not skip the busy WM8960 -- it is the only card.
    $played = Invoke-Robot (
        "amixer -c 0 sset 'Speaker Playback Off' Stereo >/dev/null 2>&1; " +
        "aplay -q -c 1 -r $Rate -f S16_LE /tmp/say.snd 2>&1 && echo PLAYED"
    )
    if ($played -notmatch 'PLAYED') {
        throw "aplay failed: $played"
    }
    if (-not $Keep) {
        Invoke-Robot 'rm -f /tmp/say.snd' | Out-Null
    }

    $seconds = [Math]::Round($size / 2.0 / $Rate, 1)
    Write-Host "gesagt (${seconds}s) ueber wm8960 unmute+aplay: `"$phrase`""
} finally {
    if ($pshell) { $pshell.Dispose() }
    if ($runspace) { $runspace.Dispose() }
    if (Test-Path -LiteralPath $work) {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

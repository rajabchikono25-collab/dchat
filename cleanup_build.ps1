# Aggressive build cleanup script for Windows file lock issues
# This script forcefully removes the target directory with retries

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "DCHAT Build Cleanup Script" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Step 1: Kill all Rust processes
Write-Host "[1/5] Killing all Rust-related processes..." -ForegroundColor Yellow
Get-Process | Where-Object {$_.ProcessName -match "cargo|rustc|rust-analyzer"} | ForEach-Object {
    Write-Host "  Killing: $($_.ProcessName) (PID: $($_.Id))" -ForegroundColor Gray
    Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Seconds 3
Write-Host "  Done!" -ForegroundColor Green
Write-Host ""

# Step 2: Verify Windows Defender exclusion
Write-Host "[2/5] Configuring Windows Defender exclusion..." -ForegroundColor Yellow
$targetPath = "C:\Users\USER\dchat\target"
try {
    Add-MpPreference -ExclusionPath $targetPath -ErrorAction Stop
    Write-Host "  Added exclusion for: $targetPath" -ForegroundColor Green
} catch {
    Write-Host "  Exclusion already exists or couldn't add (may need admin)" -ForegroundColor Yellow
}
Write-Host ""

# Step 3: Use robocopy to purge target directory (works even with locked files)
Write-Host "[3/5] Removing target directory with robocopy..." -ForegroundColor Yellow
if (Test-Path $targetPath) {
    # Create empty temp directory
    $emptyDir = "C:\Users\USER\dchat\empty_temp"
    New-Item -ItemType Directory -Path $emptyDir -Force | Out-Null
    
    # Use robocopy to mirror empty directory over target (deletes everything)
    Write-Host "  Using robocopy to purge locked files..." -ForegroundColor Gray
    robocopy $emptyDir $targetPath /MIR /R:1 /W:1 /NFL /NDL /NJH /NJS | Out-Null
    
    # Remove both directories
    Remove-Item -Path $emptyDir -Force -ErrorAction SilentlyContinue
    Remove-Item -Path $targetPath -Recurse -Force -ErrorAction SilentlyContinue
    
    if (Test-Path $targetPath) {
        Write-Host "  Warning: Some files remain locked" -ForegroundColor Yellow
    } else {
        Write-Host "  Target directory removed!" -ForegroundColor Green
    }
} else {
    Write-Host "  Target directory doesn't exist" -ForegroundColor Green
}
Write-Host ""

# Step 4: Alternative: Move to temp and schedule deletion
Write-Host "[4/5] Handling remaining locked files..." -ForegroundColor Yellow
if (Test-Path $targetPath) {
    $tempTarget = "C:\Users\USER\dchat\target_old_$(Get-Date -Format 'yyyyMMdd_HHmmss')"
    Write-Host "  Moving locked files to: $tempTarget" -ForegroundColor Gray
    try {
        Move-Item -Path $targetPath -Destination $tempTarget -Force -ErrorAction Stop
        Write-Host "  Moved! Will be cleaned up on reboot." -ForegroundColor Green
        
        # Schedule deletion on reboot
        $script = "Remove-Item -Path '$tempTarget' -Recurse -Force -ErrorAction SilentlyContinue"
        Register-ScheduledTask -TaskName "CleanupDchatTarget" -Trigger (New-ScheduledTaskTrigger -AtStartup) -Action (New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-Command `"$script; Unregister-ScheduledTask -TaskName 'CleanupDchatTarget' -Confirm:`$false`"") -Force | Out-Null
        Write-Host "  Scheduled cleanup task created for next reboot" -ForegroundColor Cyan
    } catch {
        Write-Host "  Could not move: $($_.Exception.Message)" -ForegroundColor Red
    }
}
Write-Host ""

# Step 5: Final status
Write-Host "[5/5] Final Status:" -ForegroundColor Yellow
if (Test-Path $targetPath) {
    $fileCount = (Get-ChildItem -Path $targetPath -Recurse -File -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "  Status: PARTIAL - $fileCount files remain" -ForegroundColor Yellow
    Write-Host "  Recommendation: Close all IDEs and retry, or reboot system" -ForegroundColor Yellow
} else {
    Write-Host "  Status: SUCCESS - Target directory cleaned!" -ForegroundColor Green
    Write-Host "  Ready to build!" -ForegroundColor Green
}
Write-Host ""

# Step 6: Check for remaining processes
$remainingProcs = Get-Process | Where-Object {$_.ProcessName -match "cargo|rustc|rust-analyzer"}
if ($remainingProcs) {
    Write-Host "Warning: Rust processes still running:" -ForegroundColor Red
    $remainingProcs | Select-Object ProcessName, Id | Format-Table -AutoSize
} else {
    Write-Host "No Rust processes running - good!" -ForegroundColor Green
}
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Cleanup Complete!" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

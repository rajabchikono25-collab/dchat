#!/usr/bin/env pwsh
# Test SSH connectivity to all validators using existing .pem keys

Write-Host "`n=== Testing SSH Connectivity to Validators ===" -ForegroundColor Cyan
Write-Host "Using existing .pem keys from Foundation-servers/`n" -ForegroundColor White

$validators = @(
    @{Host="validator1-ohio.schikuno.top"; IP="18.191.118.167"; Key="../Foundation-servers/AWS-Ohio/gecko.pem"; Region="Ohio"; User="ubuntu"},
    @{Host="validator1-saopaulo.schikuno.top"; IP="54.233.203.82"; Key="../Foundation-servers/AWS-Sao-Paulo/pablo.pem"; Region="São Paulo"; User="ubuntu"},
    @{Host="validator1-singapore.schikuno.top"; IP="18.142.96.209"; Key="../Foundation-servers/AWS-Singapore/craig.pem"; Region="Singapore"; User="ubuntu"},
    @{Host="validator1-stockholm.schikuno.top"; IP="13.48.49.2"; Key="../Foundation-servers/AWS-Stokholm/relay.pem"; Region="Stockholm"; User="ubuntu"},
    @{Host="validator1-india.schikuno.top"; IP="74.225.183.196"; Key="../Foundation-servers/Azure-India/uramami.pem"; Region="India"; User="azureuser"},
    @{Host="validator1-southafrica.schikuno.top"; IP="4.221.211.71"; Key="../Foundation-servers/Azure-SAfrica/anacreon.pem"; Region="South Africa"; User="azureuser"},
    @{Host="validator1-uae.schikuno.top"; IP="4.161.34.228"; Key="../Foundation-servers/Azure_UAE/Randal_key.pem"; Region="UAE"; User="azureuser"}
)

$successful = 0
$failed = 0

foreach ($v in $validators) {
    Write-Host "[$($v.Region)]" -ForegroundColor Yellow -NoNewline
    Write-Host " Testing $($v.Host)..." -ForegroundColor White
    
    # Check if key file exists
    if (-not (Test-Path $v.Key)) {
        Write-Host "  ❌ Key file not found: $($v.Key)" -ForegroundColor Red
        $failed++
        Write-Host ""
        continue
    }
    
    # Check key permissions (should be readable only by owner)
    Write-Host "  ℹ️  Key: $($v.Key)" -ForegroundColor Gray
    
    # Try SSH connection
    $sshCommand = "ssh -i `"$($v.Key)`" -o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes $($v.User)@$($v.IP) 'echo Connected'"
    
    try {
        $result = Invoke-Expression $sshCommand 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  ✅ SSH connection successful" -ForegroundColor Green
            $successful++
        } else {
            Write-Host "  ❌ SSH connection failed: $result" -ForegroundColor Red
            $failed++
        }
    } catch {
        Write-Host "  ❌ SSH connection failed: $($_.Exception.Message)" -ForegroundColor Red
        $failed++
    }
    
    Write-Host ""
}

# Summary
Write-Host "=== Summary ===" -ForegroundColor Cyan
Write-Host "Successful: $successful/7" -ForegroundColor $(if ($successful -eq 7) {"Green"} else {"Yellow"})
Write-Host "Failed: $failed/7" -ForegroundColor $(if ($failed -eq 0) {"Green"} else {"Red"})

if ($successful -eq 7) {
    Write-Host "`n✅ All validators are accessible!" -ForegroundColor Green
    Write-Host "You can now run: ansible-playbook -i inventory.ini playbook.yml" -ForegroundColor Cyan
} else {
    Write-Host "`n⚠️  Some validators are not accessible." -ForegroundColor Yellow
    Write-Host "Please check:" -ForegroundColor Gray
    Write-Host "  - Key file permissions" -ForegroundColor Gray
    Write-Host "  - SSH port (22) is open in security groups" -ForegroundColor Gray
    Write-Host "  - Correct username (ubuntu/ec2-user/admin)" -ForegroundColor Gray
    Write-Host "  - IP addresses are correct" -ForegroundColor Gray
}

Write-Host "`nDone!`n" -ForegroundColor White

#!/usr/bin/env pwsh
# DNS Verification Script for dchat Foundation Servers
# Verifies all 7 regional validator DNS records are resolving correctly

Write-Host "`n=== dchat DNS Verification ===" -ForegroundColor Cyan
Write-Host "Domain: schikuno.top" -ForegroundColor White
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`n" -ForegroundColor White

$validators = @(
    @{Region="AWS Ohio"; Host="validator1-ohio.schikuno.top"; Port=7070},
    @{Region="AWS São Paulo"; Host="validator1-saopaulo.schikuno.top"; Port=7070},
    @{Region="AWS Singapore"; Host="validator1-singapore.schikuno.top"; Port=7070},
    @{Region="AWS Stockholm"; Host="validator1-stockholm.schikuno.top"; Port=7070},
    @{Region="Azure India"; Host="validator1-india.schikuno.top"; Port=7070},
    @{Region="Azure South Africa"; Host="validator1-southafrica.schikuno.top"; Port=7070},
    @{Region="Azure UAE"; Host="validator1-uae.schikuno.top"; Port=7070}
)

$results = @()

foreach ($v in $validators) {
    Write-Host "[$($v.Region)]" -ForegroundColor Yellow -NoNewline
    Write-Host " Checking $($v.Host)..." -ForegroundColor White
    
    try {
        $dnsResult = Resolve-DnsName $v.Host -ErrorAction Stop
        $ip = $dnsResult | Where-Object {$_.Type -eq 'A'} | Select-Object -First 1 -ExpandProperty IPAddress
        
        if ($ip) {
            Write-Host "  ✅ DNS resolves to: " -ForegroundColor Green -NoNewline
            Write-Host $ip -ForegroundColor White
            
            # Try to ping the server
            $ping = Test-Connection $v.Host -Count 1 -Quiet -ErrorAction SilentlyContinue
            if ($ping) {
                Write-Host "  ✅ Server is reachable (ICMP)" -ForegroundColor Green
            } else {
                Write-Host "  ⚠️  Server not responding to ping (may be firewalled)" -ForegroundColor Yellow
            }
            
            # Try to test the port
            $portTest = Test-NetConnection $v.Host -Port $v.Port -WarningAction SilentlyContinue -InformationLevel Quiet
            if ($portTest.TcpTestSucceeded) {
                Write-Host "  ✅ Port $($v.Port) is open" -ForegroundColor Green
            } else {
                Write-Host "  ⚠️  Port $($v.Port) is not open (service not deployed yet)" -ForegroundColor Yellow
            }
            
            $results += @{
                Region = $v.Region
                Host = $v.Host
                IP = $ip
                DNSResolved = $true
                Pingable = $ping
                PortOpen = $portTest.TcpTestSucceeded
            }
        } else {
            Write-Host "  ❌ DNS resolved but no A record found" -ForegroundColor Red
            $results += @{
                Region = $v.Region
                Host = $v.Host
                IP = "N/A"
                DNSResolved = $false
                Pingable = $false
                PortOpen = $false
            }
        }
    }
    catch {
        Write-Host "  ❌ DNS resolution failed: $($_.Exception.Message)" -ForegroundColor Red
        $results += @{
            Region = $v.Region
            Host = $v.Host
            IP = "N/A"
            DNSResolved = $false
            Pingable = $false
            PortOpen = $false
        }
    }
    
    Write-Host ""
}

# Summary
Write-Host "`n=== Summary ===" -ForegroundColor Cyan
$dnsOk = ($results | Where-Object {$_.DNSResolved}).Count
$pingOk = ($results | Where-Object {$_.Pingable}).Count
$portOk = ($results | Where-Object {$_.PortOpen}).Count

Write-Host "DNS Resolution: $dnsOk/7 validators" -ForegroundColor $(if ($dnsOk -eq 7) {"Green"} else {"Yellow"})
Write-Host "Ping Response: $pingOk/7 validators" -ForegroundColor $(if ($pingOk -eq 7) {"Green"} elseif ($pingOk -eq 0) {"Yellow"} else {"Yellow"})
Write-Host "Port $($validators[0].Port) Open: $portOk/7 validators" -ForegroundColor $(if ($portOk -eq 7) {"Green"} elseif ($portOk -eq 0) {"Yellow"} else {"Yellow"})

Write-Host "`n=== Next Steps ===" -ForegroundColor Cyan
if ($dnsOk -eq 7) {
    Write-Host "✅ All DNS records are configured correctly!" -ForegroundColor Green
    if ($portOk -eq 0) {
        Write-Host "⏳ Services not yet deployed. Next: Deploy validators to servers." -ForegroundColor Yellow
        Write-Host "   See: Foundation-servers/DEPLOYMENT_CHECKLIST.md" -ForegroundColor Gray
    } elseif ($portOk -lt 7) {
        Write-Host "⚠️  Some services are deployed, others are not." -ForegroundColor Yellow
        Write-Host "   Check deployment status on each server." -ForegroundColor Gray
    } else {
        Write-Host "✅ All services are deployed and reachable!" -ForegroundColor Green
    }
} else {
    Write-Host "⚠️  Some DNS records are not resolving. Check your DNS configuration." -ForegroundColor Yellow
    Write-Host "   Records not resolving:" -ForegroundColor Gray
    $results | Where-Object {-not $_.DNSResolved} | ForEach-Object {
        Write-Host "   - $($_.Host)" -ForegroundColor Red
    }
}

Write-Host "`nFor detailed DNS propagation check, visit:" -ForegroundColor Gray
Write-Host "  https://www.whatsmydns.net/" -ForegroundColor Cyan
Write-Host "  https://dnschecker.org/" -ForegroundColor Cyan

Write-Host "`nDone!`n" -ForegroundColor White

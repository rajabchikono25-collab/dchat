#!/usr/bin/env python3
import re
import sys

# Bootstrap peers list
BOOTSTRAP_PEERS = '''bootstrap_peers = [
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWDwVLXK867iiDnB58iMMcyCqEK45WN3Y52xRPB1Cyegkj",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooW9sebMoe4meGTdXEiid7PqjcNq1dPhm32crHuWsZm97SM",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWR9vuCXZWgMTivYtpWAZqb2wcB74nYMrRqkFAHFkD7yoi",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWQ4qz5M7cvT8ri557QARdwHyFSep6EdWSnG7rRxCGXiz3",
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWNkrN3MA2y5syEwJa4nYdVW4rZi3vUN8WMPmFWV48sPZY",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWNwDFq94Rkf5koHUsbpimFApiFfGCPSMrzWoGN3xjWr85",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWMekFi9cuABuWAcStf8Ke82ZGMSnz3Qh5tv4XULHot759"
]'''

def update_bootstrap_peers(config_path):
    """Replace bootstrap_peers = [] with full list"""
    with open(config_path, 'r') as f:
        content = f.read()
    
    # Replace bootstrap_peers = [] with full array
    content = re.sub(
        r'bootstrap_peers\s*=\s*\[\s*\]',
        BOOTSTRAP_PEERS,
        content,
        flags=re.MULTILINE
    )
    
    with open(config_path, 'w') as f:
        f.write(content)
    
    print(f"✅ Updated {config_path}")

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: update-bootstrap-peers.py <config-file>")
        sys.exit(1)
    
    update_bootstrap_peers(sys.argv[1])

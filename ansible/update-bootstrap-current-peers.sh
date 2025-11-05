#!/bin/bash

# Update bootstrap peers with ACTUAL running peer IDs from the peer connection check
# These are the real peer IDs extracted from journalctl logs

cat > /tmp/bootstrap-peers-current.txt << 'EOF'
bootstrap_peers = [
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd",
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWM7sdVe45gGe4Esk2YLe4xoUoqNepfEhacvpktDRGzCT5",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS"
]
EOF

echo "Current peer IDs (from running validators):"
echo "São Paulo:    12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh"
echo "Stockholm:    12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA"
echo "Ohio:         12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit"
echo "Singapore:    12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd"
echo "India:        12D3KooWM7sdVe45gGe4Esk2YLe4xoUoqNepfEhacvpktDRGzCT5"
echo "UAE:          12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe"
echo "South Africa: 12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS"
echo ""

# Update bootstrap peers in config.template.toml
TEMPLATE="/mnt/c/Users/USER/dchat/ansible/config.template.toml"

if [ -f "$TEMPLATE" ]; then
    echo "Updating $TEMPLATE with current peer IDs..."
    # Use Python for safe replacement
    python3 << 'PYTHON'
import re

template_path = "/mnt/c/Users/USER/dchat/ansible/config.template.toml"

with open(template_path, 'r') as f:
    content = f.read()

# New bootstrap peers with correct peer IDs
new_peers = '''bootstrap_peers = [
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd",
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWM7sdVe45gGe4Esk2YLe4xoUoqNepfEhacvpktDRGzCT5",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS"
]'''

# Replace bootstrap_peers array
pattern = r'bootstrap_peers\s*=\s*\[[^\]]*\]'
content = re.sub(pattern, new_peers, content, flags=re.MULTILINE | re.DOTALL)

with open(template_path, 'w') as f:
    f.write(content)

print("✓ Template updated with current peer IDs")
PYTHON
else
    echo "ERROR: Template not found at $TEMPLATE"
    exit 1
fi

echo ""
echo "Next steps:"
echo "1. Deploy updated configs to all validators"
echo "2. Restart dchat service on all validators"
echo ""
echo "Run: wsl bash /mnt/c/Users/USER/dchat/ansible/deploy-and-restart-all.sh"

"""
Unit tests for WebSocketClient
"""

import pytest
import asyncio
from dchat.messaging import WebSocketClient, ConnectionState


@pytest.mark.asyncio
class TestWebSocketClient:
    """Test suite for WebSocketClient"""

    def test_initialization(self):
        """Test WebSocket client initialization"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        assert client.url == "wss://relay.dchat.network"
        assert client.state == ConnectionState.DISCONNECTED
        assert not client.is_connected
        assert client.reconnect_delay == 3
        assert client.max_reconnect_attempts == 5
        assert client.ping_interval == 30

    def test_custom_settings(self):
        """Test custom reconnection and ping settings"""
        client = WebSocketClient(
            url="wss://relay.dchat.network",
            reconnect_delay=5,
            max_reconnect_attempts=10,
            ping_interval=60,
        )
        
        assert client.reconnect_delay == 5
        assert client.max_reconnect_attempts == 10
        assert client.ping_interval == 60

    def test_event_handlers(self):
        """Test event handler registration"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        message_received = []
        errors_received = []
        connected = []
        disconnected = []
        
        client.on_message(lambda data: message_received.append(data))
        client.on_error(lambda error: errors_received.append(error))
        client.on_connect(lambda: connected.append(True))
        client.on_disconnect(lambda: disconnected.append(True))
        
        # Handlers should be registered
        assert len(client._message_handlers) == 1
        assert len(client._error_handlers) == 1
        assert len(client._connect_handlers) == 1
        assert len(client._disconnect_handlers) == 1

    def test_multiple_handlers(self):
        """Test multiple handlers for same event"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        handler1_called = []
        handler2_called = []
        
        client.on_message(lambda data: handler1_called.append(data))
        client.on_message(lambda data: handler2_called.append(data))
        
        assert len(client._message_handlers) == 2

    @pytest.mark.asyncio
    async def test_send_without_connection(self):
        """Test sending message without connection raises error"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        with pytest.raises(RuntimeError, match="not connected"):
            await client.send({"type": "test"})

    @pytest.mark.asyncio
    async def test_connection_state_tracking(self):
        """Test connection state transitions"""
        client = WebSocketClient(url="wss://echo.websocket.org")
        
        assert client.state == ConnectionState.DISCONNECTED
        
        # Note: Full connection test requires WebSocket server
        # This is a basic state test

    @pytest.mark.asyncio
    async def test_dispose_cleanup(self):
        """Test resource disposal"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        client.on_message(lambda data: None)
        client.on_error(lambda error: None)
        
        await client.dispose()
        
        # Handlers should be cleared
        assert len(client._message_handlers) == 0
        assert len(client._error_handlers) == 0
        assert len(client._connect_handlers) == 0
        assert len(client._disconnect_handlers) == 0

    @pytest.mark.asyncio
    async def test_context_manager(self):
        """Test async context manager protocol"""
        # This would require a test WebSocket server
        # Placeholder for context manager test
        pass

    @pytest.mark.asyncio
    async def test_disconnect_cleanup(self):
        """Test disconnect cleans up tasks"""
        client = WebSocketClient(url="wss://relay.dchat.network")
        
        # Should not raise error even if not connected
        await client.disconnect()
        
        assert client.state == ConnectionState.DISCONNECTED


@pytest.mark.asyncio
class TestWebSocketClientIntegration:
    """Integration tests for WebSocketClient (requires test server)"""

    @pytest.mark.skip(reason="Requires test WebSocket server")
    async def test_connect_and_send(self):
        """Test connection and message sending"""
        client = WebSocketClient(url="wss://echo.websocket.org")
        
        messages_received = []
        client.on_message(lambda data: messages_received.append(data))
        
        await client.connect()
        await asyncio.sleep(1)  # Wait for connection
        
        await client.send({"type": "test", "content": "hello"})
        await asyncio.sleep(1)  # Wait for echo
        
        await client.disconnect()
        
        # Echo server should return our message
        assert len(messages_received) > 0

    @pytest.mark.skip(reason="Requires test WebSocket server")
    async def test_reconnection_on_disconnect(self):
        """Test automatic reconnection"""
        client = WebSocketClient(
            url="wss://echo.websocket.org",
            reconnect_delay=1,
            max_reconnect_attempts=3,
        )
        
        reconnect_attempts = []
        client.on_connect(lambda: reconnect_attempts.append(True))
        
        await client.connect()
        await asyncio.sleep(1)
        
        # Simulate disconnect
        if client._ws:
            await client._ws.close()
        
        # Wait for reconnection attempts
        await asyncio.sleep(5)
        
        await client.dispose()

    @pytest.mark.skip(reason="Requires test WebSocket server")
    async def test_ping_heartbeat(self):
        """Test periodic ping mechanism"""
        client = WebSocketClient(
            url="wss://echo.websocket.org",
            ping_interval=1,
        )
        
        await client.connect()
        await asyncio.sleep(3)  # Wait for multiple pings
        
        assert client.is_connected
        
        await client.disconnect()


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

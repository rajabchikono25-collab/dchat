"""
WebSocket client for real-time relay connections
"""

import asyncio
import json
from enum import Enum
from typing import Optional, Callable, Any, Dict, List
import aiohttp


class ConnectionState(Enum):
    """WebSocket connection states"""
    DISCONNECTED = "disconnected"
    CONNECTING = "connecting"
    CONNECTED = "connected"
    RECONNECTING = "reconnecting"
    FAILED = "failed"


class WebSocketClient:
    """
    WebSocket client for real-time bidirectional communication with dchat relays.
    
    Features:
    - Automatic reconnection with exponential backoff
    - Heartbeat/ping mechanism for connection health
    - Event-driven architecture with multiple handler support
    - JSON message encoding/decoding
    - Graceful connection cleanup
    
    Example:
        ```python
        async def main():
            ws = WebSocketClient(
                url="wss://relay.dchat.network",
                reconnect_delay=3,
                max_reconnect_attempts=5,
                ping_interval=30
            )
            
            ws.on_message(lambda data: print(f"Received: {data}"))
            ws.on_connect(lambda: print("Connected!"))
            
            await ws.connect()
            await ws.send({"type": "message", "content": "Hello!"})
            
            # Keep alive
            await asyncio.sleep(60)
            await ws.disconnect()
        ```
    """

    def __init__(
        self,
        url: str,
        reconnect_delay: int = 3,
        max_reconnect_attempts: int = 5,
        ping_interval: int = 30,
    ):
        self.url = url
        self.reconnect_delay = reconnect_delay
        self.max_reconnect_attempts = max_reconnect_attempts
        self.ping_interval = ping_interval
        
        self._state = ConnectionState.DISCONNECTED
        self._ws: Optional[aiohttp.ClientWebSocketResponse] = None
        self._session: Optional[aiohttp.ClientSession] = None
        self._reconnect_count = 0
        self._ping_task: Optional[asyncio.Task] = None
        self._receive_task: Optional[asyncio.Task] = None
        
        # Event handlers
        self._message_handlers: List[Callable[[Dict[str, Any]], None]] = []
        self._error_handlers: List[Callable[[Exception], None]] = []
        self._connect_handlers: List[Callable[[], None]] = []
        self._disconnect_handlers: List[Callable[[], None]] = []

    @property
    def is_connected(self) -> bool:
        """Check if WebSocket is connected"""
        return self._state == ConnectionState.CONNECTED and self._ws is not None and not self._ws.closed

    @property
    def state(self) -> ConnectionState:
        """Get current connection state"""
        return self._state

    def on_message(self, handler: Callable[[Dict[str, Any]], None]) -> None:
        """Register a message handler"""
        self._message_handlers.append(handler)

    def on_error(self, handler: Callable[[Exception], None]) -> None:
        """Register an error handler"""
        self._error_handlers.append(handler)

    def on_connect(self, handler: Callable[[], None]) -> None:
        """Register a connection handler"""
        self._connect_handlers.append(handler)

    def on_disconnect(self, handler: Callable[[], None]) -> None:
        """Register a disconnection handler"""
        self._disconnect_handlers.append(handler)

    async def connect(self) -> None:
        """Connect to WebSocket relay"""
        if self.is_connected:
            return

        self._state = ConnectionState.CONNECTING
        
        try:
            if not self._session:
                self._session = aiohttp.ClientSession()
            
            self._ws = await self._session.ws_connect(self.url)
            self._state = ConnectionState.CONNECTED
            self._reconnect_count = 0
            
            # Start ping task
            self._ping_task = asyncio.create_task(self._ping_loop())
            
            # Start receive task
            self._receive_task = asyncio.create_task(self._receive_loop())
            
            # Notify connection handlers
            for handler in self._connect_handlers:
                try:
                    handler()
                except Exception as e:
                    await self._handle_error(e)
                    
        except Exception as e:
            self._state = ConnectionState.FAILED
            await self._handle_error(e)
            await self._schedule_reconnect()

    async def disconnect(self) -> None:
        """Disconnect from WebSocket relay"""
        if self._ws and not self._ws.closed:
            await self._ws.close()
        
        # Cancel tasks
        if self._ping_task:
            self._ping_task.cancel()
            try:
                await self._ping_task
            except asyncio.CancelledError:
                pass
        
        if self._receive_task:
            self._receive_task.cancel()
            try:
                await self._receive_task
            except asyncio.CancelledError:
                pass
        
        self._state = ConnectionState.DISCONNECTED
        
        # Notify disconnect handlers
        for handler in self._disconnect_handlers:
            try:
                handler()
            except Exception as e:
                await self._handle_error(e)

    async def send(self, data: Dict[str, Any]) -> None:
        """Send JSON message via WebSocket"""
        if not self.is_connected:
            raise RuntimeError("WebSocket not connected")
        
        try:
            message = json.dumps(data)
            await self._ws.send_str(message)
        except Exception as e:
            await self._handle_error(e)
            raise

    async def _receive_loop(self) -> None:
        """Continuously receive messages from WebSocket"""
        try:
            async for msg in self._ws:
                if msg.type == aiohttp.WSMsgType.TEXT:
                    try:
                        data = json.loads(msg.data)
                        # Notify message handlers
                        for handler in self._message_handlers:
                            try:
                                handler(data)
                            except Exception as e:
                                await self._handle_error(e)
                    except json.JSONDecodeError as e:
                        await self._handle_error(e)
                        
                elif msg.type == aiohttp.WSMsgType.ERROR:
                    await self._handle_error(Exception(f"WebSocket error: {self._ws.exception()}"))
                    break
                    
                elif msg.type in (aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.CLOSING):
                    break
                    
        except Exception as e:
            await self._handle_error(e)
        finally:
            if self._state == ConnectionState.CONNECTED:
                await self._schedule_reconnect()

    async def _ping_loop(self) -> None:
        """Send periodic pings to keep connection alive"""
        try:
            while self.is_connected:
                await asyncio.sleep(self.ping_interval)
                if self.is_connected:
                    await self._ws.ping()
        except asyncio.CancelledError:
            pass
        except Exception as e:
            await self._handle_error(e)

    async def _schedule_reconnect(self) -> None:
        """Schedule reconnection attempt"""
        if self._reconnect_count >= self.max_reconnect_attempts:
            self._state = ConnectionState.FAILED
            return
        
        self._state = ConnectionState.RECONNECTING
        self._reconnect_count += 1
        
        # Exponential backoff
        delay = self.reconnect_delay * (2 ** (self._reconnect_count - 1))
        await asyncio.sleep(delay)
        
        await self.connect()

    async def _handle_error(self, error: Exception) -> None:
        """Handle errors and notify error handlers"""
        for handler in self._error_handlers:
            try:
                handler(error)
            except Exception:
                pass  # Prevent cascading errors

    async def dispose(self) -> None:
        """Cleanup resources"""
        await self.disconnect()
        
        if self._session:
            await self._session.close()
            self._session = None
        
        self._message_handlers.clear()
        self._error_handlers.clear()
        self._connect_handlers.clear()
        self._disconnect_handlers.clear()

    async def __aenter__(self):
        await self.connect()
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.dispose()

"""
User management for creating users and handling profiles
"""

import uuid
from datetime import datetime, timezone
from typing import Optional, List, Dict, Any

from ..blockchain.client import BlockchainClient
from ..crypto.keypair import KeyPair, hash_content
from ..messaging import WebSocketClient, HttpClient
from .models import (
    CreateUserResponse,
    DirectMessageResponse,
    CreateChannelResponse,
    UserProfile,
    DirectMessage,
    ChannelMessage,
)


class UserManager:
    """User manager for user operations"""

    def __init__(self, blockchain: BlockchainClient, base_url: str):
        self.blockchain = blockchain
        self.base_url = base_url
        self._http_client = HttpClient(base_url=base_url)
        self._ws_client: Optional[WebSocketClient] = None

    async def create_user(self, username: str) -> CreateUserResponse:
        """Create a new user with blockchain registration"""
        # Generate unique user ID
        user_id = str(uuid.uuid4())

        # Generate Ed25519 key pair
        keypair = KeyPair.generate()

        # Submit blockchain transaction
        tx_id = await self.blockchain.register_user(
            user_id=user_id,
            username=username,
            public_key=keypair.public_key_hex,
        )

        # Wait for blockchain confirmation
        receipt = await self.blockchain.wait_for_confirmation(tx_id)
        on_chain_confirmed = receipt.success

        # Return response with actual blockchain status
        return CreateUserResponse(
            user_id=user_id,
            username=username,
            public_key=keypair.public_key_hex,
            private_key=keypair.private_key_hex,
            created_at=datetime.now(timezone.utc).isoformat(),
            on_chain_confirmed=on_chain_confirmed,
            tx_id=tx_id,
        )

    async def send_direct_message(
        self,
        sender_id: str,
        recipient_id: str,
        content: str,
        relay_node_id: str = None,
    ) -> DirectMessageResponse:
        """Send a direct message"""
        # Generate message ID
        message_id = str(uuid.uuid4())

        # Hash the content
        content_hash = hash_content(content)

        # Submit blockchain transaction
        tx_id = await self.blockchain.send_direct_message(
            message_id=message_id,
            sender_id=sender_id,
            recipient_id=recipient_id,
            content_hash=content_hash,
            payload_size=len(content),
            relay_node_id=relay_node_id,
        )

        # Wait for confirmation
        receipt = await self.blockchain.wait_for_confirmation(tx_id)
        on_chain_confirmed = receipt.success

        return DirectMessageResponse(
            message_id=message_id,
            sender_id=sender_id,
            recipient_id=recipient_id,
            content_hash=content_hash,
            created_at=datetime.now(timezone.utc).isoformat(),
            on_chain_confirmed=on_chain_confirmed,
            tx_id=tx_id,
        )

    async def create_channel(
        self,
        creator_id: str,
        channel_name: str,
        description: str = None,
    ) -> CreateChannelResponse:
        """Create a new channel"""
        # Generate channel ID
        channel_id = str(uuid.uuid4())

        # Submit blockchain transaction
        tx_id = await self.blockchain.create_channel(
            channel_id=channel_id,
            name=channel_name,
            description=description or "",
            creator_id=creator_id,
        )

        # Wait for confirmation
        receipt = await self.blockchain.wait_for_confirmation(tx_id)
        on_chain_confirmed = receipt.success

        return CreateChannelResponse(
            channel_id=channel_id,
            name=channel_name,
            description=description,
            creator_id=creator_id,
            created_at=datetime.now(timezone.utc).isoformat(),
            on_chain_confirmed=on_chain_confirmed,
            tx_id=tx_id,
        )

    async def post_to_channel(
        self,
        sender_id: str,
        channel_id: str,
        content: str,
    ) -> DirectMessageResponse:
        """Post a message to a channel"""
        # Generate message ID
        message_id = str(uuid.uuid4())

        # Hash the content
        content_hash = hash_content(content)

        # Submit blockchain transaction
        tx_id = await self.blockchain.post_to_channel(
            message_id=message_id,
            channel_id=channel_id,
            sender_id=sender_id,
            content_hash=content_hash,
            payload_size=len(content),
        )

        # Wait for confirmation
        receipt = await self.blockchain.wait_for_confirmation(tx_id)
        on_chain_confirmed = receipt.success

        return DirectMessageResponse(
            message_id=message_id,
            sender_id=sender_id,
            recipient_id=channel_id,  # Using channel_id as recipient
            content_hash=content_hash,
            created_at=datetime.now(timezone.utc).isoformat(),
            on_chain_confirmed=on_chain_confirmed,
            tx_id=tx_id,
        )

    async def get_user_profile(self, user_id: str) -> Optional[UserProfile]:
        """
        Get user profile by user ID.
        Queries HTTP API for user registration and profile data.
        """
        try:
            profile_data = await self._http_client.get(f"/api/users/{user_id}")
            
            if profile_data:
                return UserProfile(
                    user_id=user_id,
                    username=profile_data["username"],
                    public_key=profile_data["publicKey"],
                    created_at=profile_data["createdAt"],
                    reputation=profile_data.get("reputation", 0),
                    on_chain_confirmed=profile_data.get("onChainConfirmed", False),
                )
            
            return None
        except Exception:
            return None

    async def get_direct_messages(
        self, user_id: str, limit: int = 50
    ) -> List[DirectMessage]:
        """Get direct messages for a user"""
        try:
            response = await self._http_client.get(
                f"/api/users/{user_id}/messages",
                query_params={"limit": str(limit)},
            )
            
            if not response or "messages" not in response:
                return []
            
            messages = response["messages"]
            return [
                DirectMessage(
                    message_id=msg["messageId"],
                    sender_id=msg["senderId"],
                    recipient_id=msg["recipientId"],
                    content=msg["content"],
                    content_hash=msg["contentHash"],
                    created_at=msg["createdAt"],
                    on_chain_confirmed=msg.get("onChainConfirmed", False),
                )
                for msg in messages
            ]
        except Exception:
            return []

    async def get_channel_messages(
        self, channel_id: str, limit: int = 50
    ) -> List[ChannelMessage]:
        """Get channel messages"""
        try:
            response = await self._http_client.get(
                f"/api/channels/{channel_id}/messages",
                query_params={"limit": str(limit)},
            )
            
            if not response or "messages" not in response:
                return []
            
            messages = response["messages"]
            return [
                ChannelMessage(
                    message_id=msg["messageId"],
                    channel_id=msg["channelId"],
                    sender_id=msg["senderId"],
                    content=msg["content"],
                    content_hash=msg["contentHash"],
                    created_at=msg["createdAt"],
                    on_chain_confirmed=msg.get("onChainConfirmed", False),
                )
                for msg in messages
            ]
        except Exception:
            return []

    async def connect_to_relay(self, relay_url: str) -> None:
        """Connect to WebSocket relay for real-time messaging"""
        self._ws_client = WebSocketClient(url=relay_url)
        await self._ws_client.connect()

    async def disconnect_from_relay(self) -> None:
        """Disconnect from WebSocket relay"""
        if self._ws_client:
            await self._ws_client.disconnect()
            self._ws_client = None

    @property
    def is_connected_to_relay(self) -> bool:
        """Check if connected to relay"""
        return self._ws_client is not None and self._ws_client.is_connected

    async def send_realtime_message(self, message: Dict[str, Any]) -> None:
        """Send real-time message via WebSocket"""
        if not self.is_connected_to_relay:
            raise RuntimeError("Not connected to relay")
        await self._ws_client.send(message)

    def on_realtime_message(self, handler) -> None:
        """Register message handler for real-time messages"""
        if self._ws_client:
            self._ws_client.on_message(handler)

    async def dispose(self) -> None:
        """Dispose resources"""
        await self._http_client.close()
        if self._ws_client:
            await self._ws_client.dispose()

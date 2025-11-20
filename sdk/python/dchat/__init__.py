"""
dchat Python SDK

Official SDK for building decentralized chat applications with blockchain integration.
"""

from .blockchain.client import BlockchainClient, BlockchainConfig
from .blockchain.transaction import (
    TransactionStatus,
    TransactionReceipt,
    ChannelVisibility,
    RegisterUserTx,
    SendDirectMessageTx,
    CreateChannelTx,
    PostToChannelTx,
)
from .user.manager import UserManager
from .user.models import (
    CreateUserResponse,
    UserProfile,
    DirectMessageResponse,
    CreateChannelResponse,
    ChannelMessage,
    DirectMessage,
)
from .crypto.keypair import KeyPair, hash_content
from .messaging import WebSocketClient, HttpClient, ConnectionState, HttpException

__version__ = "0.1.0"
__all__ = [
    # Blockchain
    "BlockchainClient",
    "BlockchainConfig",
    "TransactionStatus",
    "TransactionReceipt",
    "ChannelVisibility",
    "RegisterUserTx",
    "SendDirectMessageTx",
    "CreateChannelTx",
    "PostToChannelTx",
    # User management
    "UserManager",
    "CreateUserResponse",
    "UserProfile",
    "DirectMessageResponse",
    "CreateChannelResponse",
    "ChannelMessage",
    "DirectMessage",
    # Crypto
    "KeyPair",
    "hash_content",
    # Messaging
    "WebSocketClient",
    "HttpClient",
    "ConnectionState",
    "HttpException",
]

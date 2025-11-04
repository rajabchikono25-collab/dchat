"""
Cryptographic utilities for key management
Uses PyNaCl for real Ed25519 cryptography
"""

import hashlib
from typing import Tuple

try:
    from nacl.signing import SigningKey, VerifyKey
    from nacl.encoding import RawEncoder
    PYNACL_AVAILABLE = True
except ImportError:
    PYNACL_AVAILABLE = False
    import os


class KeyPair:
    """Ed25519 key pair for identity management using PyNaCl"""

    def __init__(self, public_key: bytes, private_key: bytes):
        self.public_key = public_key
        self.private_key = private_key
        if PYNACL_AVAILABLE:
            self._signing_key = SigningKey(private_key, encoder=RawEncoder)
            self._verify_key = VerifyKey(public_key, encoder=RawEncoder)

    @classmethod
    def generate(cls) -> "KeyPair":
        """Generate a new random Ed25519 key pair"""
        if not PYNACL_AVAILABLE:
            raise ImportError("PyNaCl is required for Ed25519 cryptography. Install: pip install PyNaCl")
        
        # Generate Ed25519 key pair using PyNaCl
        signing_key = SigningKey.generate()
        verify_key = signing_key.verify_key
        
        private_key = bytes(signing_key)
        public_key = bytes(verify_key)
        
        return cls(public_key, private_key)

    @classmethod
    def from_private_key(cls, private_key: bytes) -> "KeyPair":
        """Create from existing private key"""
        if not PYNACL_AVAILABLE:
            raise ImportError("PyNaCl is required for Ed25519 cryptography. Install: pip install PyNaCl")
        
        # Derive public key from private key using Ed25519
        signing_key = SigningKey(private_key, encoder=RawEncoder)
        public_key = bytes(signing_key.verify_key)
        
        return cls(public_key, private_key)

    @property
    def public_key_hex(self) -> str:
        """Get public key as hex string"""
        return self.public_key.hex()

    @property
    def private_key_hex(self) -> str:
        """Get private key as hex string"""
        return self.private_key.hex()

    def sign(self, message: bytes) -> bytes:
        """Sign a message using Ed25519"""
        if not PYNACL_AVAILABLE:
            raise ImportError("PyNaCl is required for Ed25519 signing")
        
        # Sign message using Ed25519
        signed = self._signing_key.sign(message, encoder=RawEncoder)
        # Return only the signature (first 64 bytes)
        return signed.signature

    def verify(self, message: bytes, signature: bytes) -> bool:
        """Verify an Ed25519 signature"""
        if not PYNACL_AVAILABLE:
            return False
        
        try:
            # Verify signature using Ed25519
            self._verify_key.verify(message, signature, encoder=RawEncoder)
            return True
        except Exception:
            return False

    def to_dict(self) -> dict:
        """Export key pair to dictionary"""
        return {
            "public_key": self.public_key_hex,
            "private_key": self.private_key_hex,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "KeyPair":
        """Import key pair from dictionary"""
        return cls(
            public_key=bytes.fromhex(data["public_key"]),
            private_key=bytes.fromhex(data["private_key"]),
        )


def hash_content(content: str) -> str:
    """Hash content using SHA-256"""
    return hashlib.sha256(content.encode()).hexdigest()


def hash_bytes(data: bytes) -> str:
    """Hash bytes using SHA-256"""
    return hashlib.sha256(data).hexdigest()


def bytes_to_hex(data: bytes) -> str:
    """Convert bytes to hex string"""
    return data.hex()


def hex_to_bytes(hex_str: str) -> bytes:
    """Convert hex string to bytes"""
    return bytes.fromhex(hex_str)

"""
HTTP client for REST API calls
"""

from typing import Optional, Dict, Any
import aiohttp


class HttpException(Exception):
    """HTTP request exception with status code"""
    
    def __init__(self, message: str, status_code: Optional[int] = None, body: Optional[str] = None):
        super().__init__(message)
        self.message = message
        self.status_code = status_code
        self.body = body
    
    def __str__(self) -> str:
        if self.status_code:
            return f"HttpException: {self.message} (Status: {self.status_code})"
        return f"HttpException: {self.message}"


class HttpClient:
    """
    HTTP client wrapper for dchat REST API calls.
    
    Features:
    - GET, POST, PUT, DELETE methods
    - Query parameter support
    - Custom headers and timeout
    - JSON request/response handling
    - Structured error handling
    - 404 responses return None for optional data
    
    Example:
        ```python
        async def main():
            client = HttpClient(
                base_url="https://api.dchat.network",
                timeout=30,
                headers={"Authorization": "Bearer token"}
            )
            
            # GET request
            user_data = await client.get("/api/users/alice")
            
            # GET with query params
            messages = await client.get(
                "/api/users/alice/messages",
                query_params={"limit": "50"}
            )
            
            # POST request
            result = await client.post(
                "/api/channels",
                body={"name": "general", "description": "General discussion"}
            )
            
            # Cleanup
            await client.close()
        ```
    """
    
    def __init__(
        self,
        base_url: str,
        timeout: int = 30,
        headers: Optional[Dict[str, str]] = None,
    ):
        self.base_url = base_url.rstrip("/")
        self.timeout = aiohttp.ClientTimeout(total=timeout)
        self.default_headers = {
            "Content-Type": "application/json",
            **(headers or {}),
        }
        self._session: Optional[aiohttp.ClientSession] = None

    async def _get_session(self) -> aiohttp.ClientSession:
        """Get or create aiohttp session"""
        if not self._session:
            self._session = aiohttp.ClientSession(
                timeout=self.timeout,
                headers=self.default_headers,
            )
        return self._session

    def _build_url(self, path: str, query_params: Optional[Dict[str, str]] = None) -> str:
        """Build full URL with query parameters"""
        url = f"{self.base_url}{path}"
        
        if query_params:
            # Build query string
            query_parts = [f"{k}={v}" for k, v in query_params.items()]
            url = f"{url}?{'&'.join(query_parts)}"
        
        return url

    async def get(
        self,
        path: str,
        query_params: Optional[Dict[str, str]] = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Optional[Dict[str, Any]]:
        """
        Make GET request.
        Returns None if 404, otherwise returns JSON response.
        """
        session = await self._get_session()
        url = self._build_url(path, query_params)
        
        try:
            async with session.get(url, headers=headers) as response:
                return await self._handle_response(response)
        except aiohttp.ClientError as e:
            raise HttpException(f"GET request failed: {str(e)}")

    async def post(
        self,
        path: str,
        body: Optional[Dict[str, Any]] = None,
        query_params: Optional[Dict[str, str]] = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Optional[Dict[str, Any]]:
        """Make POST request"""
        session = await self._get_session()
        url = self._build_url(path, query_params)
        
        try:
            async with session.post(url, json=body, headers=headers) as response:
                return await self._handle_response(response)
        except aiohttp.ClientError as e:
            raise HttpException(f"POST request failed: {str(e)}")

    async def put(
        self,
        path: str,
        body: Optional[Dict[str, Any]] = None,
        query_params: Optional[Dict[str, str]] = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Optional[Dict[str, Any]]:
        """Make PUT request"""
        session = await self._get_session()
        url = self._build_url(path, query_params)
        
        try:
            async with session.put(url, json=body, headers=headers) as response:
                return await self._handle_response(response)
        except aiohttp.ClientError as e:
            raise HttpException(f"PUT request failed: {str(e)}")

    async def delete(
        self,
        path: str,
        query_params: Optional[Dict[str, str]] = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Optional[Dict[str, Any]]:
        """Make DELETE request"""
        session = await self._get_session()
        url = self._build_url(path, query_params)
        
        try:
            async with session.delete(url, headers=headers) as response:
                return await self._handle_response(response)
        except aiohttp.ClientError as e:
            raise HttpException(f"DELETE request failed: {str(e)}")

    async def _handle_response(self, response: aiohttp.ClientResponse) -> Optional[Dict[str, Any]]:
        """Handle HTTP response"""
        # 404 returns None for optional data
        if response.status == 404:
            return None
        
        # Success status codes
        if 200 <= response.status < 300:
            try:
                return await response.json()
            except Exception:
                # Empty response
                return {}
        
        # Error status codes
        body = await response.text()
        raise HttpException(
            f"HTTP {response.status}: {response.reason}",
            status_code=response.status,
            body=body,
        )

    async def close(self) -> None:
        """Close HTTP session"""
        if self._session:
            await self._session.close()
            self._session = None

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.close()

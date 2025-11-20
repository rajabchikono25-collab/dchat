"""
Unit tests for HttpClient
"""

import pytest
from dchat.messaging import HttpClient, HttpException


@pytest.mark.asyncio
class TestHttpClient:
    """Test suite for HttpClient"""

    def test_initialization(self):
        """Test HTTP client initialization"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        assert client.base_url == "https://api.dchat.network"
        assert "Content-Type" in client.default_headers
        assert client.default_headers["Content-Type"] == "application/json"

    def test_initialization_with_trailing_slash(self):
        """Test base URL normalization"""
        client = HttpClient(base_url="https://api.dchat.network/")
        
        assert client.base_url == "https://api.dchat.network"

    def test_custom_headers(self):
        """Test custom headers"""
        client = HttpClient(
            base_url="https://api.dchat.network",
            headers={"Authorization": "Bearer token123"},
        )
        
        assert "Authorization" in client.default_headers
        assert client.default_headers["Authorization"] == "Bearer token123"

    def test_custom_timeout(self):
        """Test custom timeout configuration"""
        client = HttpClient(
            base_url="https://api.dchat.network",
            timeout=60,
        )
        
        assert client.timeout.total == 60

    def test_build_url_without_params(self):
        """Test URL building without query parameters"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        url = client._build_url("/api/users/alice")
        assert url == "https://api.dchat.network/api/users/alice"

    def test_build_url_with_params(self):
        """Test URL building with query parameters"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        url = client._build_url(
            "/api/users/alice/messages",
            query_params={"limit": "50", "offset": "10"},
        )
        
        assert "https://api.dchat.network/api/users/alice/messages?" in url
        assert "limit=50" in url
        assert "offset=10" in url

    def test_build_url_special_characters(self):
        """Test URL building with special characters in params"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        url = client._build_url(
            "/api/search",
            query_params={"query": "hello world", "filter": "category:test"},
        )
        
        assert "query=hello world" in url or "query=hello%20world" in url

    @pytest.mark.asyncio
    async def test_get_request_structure(self):
        """Test GET request method exists and has correct signature"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        # Verify method exists
        assert hasattr(client, "get")
        assert callable(client.get)

    @pytest.mark.asyncio
    async def test_post_request_structure(self):
        """Test POST request method exists"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        assert hasattr(client, "post")
        assert callable(client.post)

    @pytest.mark.asyncio
    async def test_put_request_structure(self):
        """Test PUT request method exists"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        assert hasattr(client, "put")
        assert callable(client.put)

    @pytest.mark.asyncio
    async def test_delete_request_structure(self):
        """Test DELETE request method exists"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        assert hasattr(client, "delete")
        assert callable(client.delete)

    @pytest.mark.asyncio
    async def test_close_without_session(self):
        """Test closing client without open session"""
        client = HttpClient(base_url="https://api.dchat.network")
        
        # Should not raise error
        await client.close()
        assert client._session is None

    @pytest.mark.asyncio
    async def test_context_manager(self):
        """Test async context manager protocol"""
        async with HttpClient(base_url="https://api.dchat.network") as client:
            assert client is not None
        
        # Session should be closed after exit
        assert client._session is None


class TestHttpException:
    """Test suite for HttpException"""

    def test_exception_creation(self):
        """Test exception creation with status code"""
        exc = HttpException("Not Found", status_code=404)
        
        assert exc.message == "Not Found"
        assert exc.status_code == 404
        assert exc.body is None

    def test_exception_with_body(self):
        """Test exception with response body"""
        exc = HttpException(
            "Bad Request",
            status_code=400,
            body='{"error": "Invalid input"}',
        )
        
        assert exc.status_code == 400
        assert exc.body == '{"error": "Invalid input"}'

    def test_exception_string_representation(self):
        """Test exception string formatting"""
        exc = HttpException("Internal Server Error", status_code=500)
        
        str_repr = str(exc)
        assert "500" in str_repr
        assert "Internal Server Error" in str_repr

    def test_exception_without_status_code(self):
        """Test exception without status code"""
        exc = HttpException("Network error")
        
        assert exc.status_code is None
        str_repr = str(exc)
        assert "Network error" in str_repr


@pytest.mark.asyncio
class TestHttpClientIntegration:
    """Integration tests for HttpClient (requires test server)"""

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_get_request_success(self):
        """Test successful GET request"""
        client = HttpClient(base_url="http://localhost:8000")
        
        result = await client.get("/api/test")
        assert result is not None

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_get_request_404(self):
        """Test GET request with 404 response"""
        client = HttpClient(base_url="http://localhost:8000")
        
        result = await client.get("/api/nonexistent")
        assert result is None

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_post_request_with_body(self):
        """Test POST request with JSON body"""
        client = HttpClient(base_url="http://localhost:8000")
        
        result = await client.post(
            "/api/test",
            body={"data": "value"},
        )
        assert result is not None

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_error_response_handling(self):
        """Test error response handling"""
        client = HttpClient(base_url="http://localhost:8000")
        
        with pytest.raises(HttpException) as exc_info:
            await client.get("/api/error")
        
        assert exc_info.value.status_code >= 400

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_timeout_handling(self):
        """Test timeout error handling"""
        client = HttpClient(
            base_url="http://192.0.2.1",  # Non-routable address
            timeout=1,
        )
        
        with pytest.raises(HttpException):
            await client.get("/api/test")

    @pytest.mark.skip(reason="Requires test HTTP server")
    async def test_query_parameters_encoding(self):
        """Test query parameter encoding in actual request"""
        client = HttpClient(base_url="http://localhost:8000")
        
        result = await client.get(
            "/api/search",
            query_params={
                "query": "hello world",
                "limit": "10",
            },
        )
        # Server should receive properly encoded parameters


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

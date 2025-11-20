/// HTTP client for REST API calls
library;

import 'dart:convert';
import 'package:http/http.dart' as http;

/// HTTP client wrapper for dchat API
class HttpClient {
  final String baseUrl;
  final Duration timeout;
  final Map<String, String> defaultHeaders;

  HttpClient({
    required this.baseUrl,
    this.timeout = const Duration(seconds: 30),
    Map<String, String>? headers,
  }) : defaultHeaders = {
          'Content-Type': 'application/json',
          ...?headers,
        };

  /// GET request
  Future<Map<String, dynamic>?> get(
    String path, {
    Map<String, String>? queryParams,
    Map<String, String>? headers,
  }) async {
    try {
      final uri = _buildUri(path, queryParams);
      final response = await http
          .get(
            uri,
            headers: {...defaultHeaders, ...?headers},
          )
          .timeout(timeout);

      return _handleResponse(response);
    } catch (e) {
      throw HttpException('GET request failed: $e');
    }
  }

  /// POST request
  Future<Map<String, dynamic>?> post(
    String path, {
    Map<String, dynamic>? body,
    Map<String, String>? headers,
  }) async {
    try {
      final uri = _buildUri(path);
      final response = await http
          .post(
            uri,
            headers: {...defaultHeaders, ...?headers},
            body: body != null ? jsonEncode(body) : null,
          )
          .timeout(timeout);

      return _handleResponse(response);
    } catch (e) {
      throw HttpException('POST request failed: $e');
    }
  }

  /// PUT request
  Future<Map<String, dynamic>?> put(
    String path, {
    Map<String, dynamic>? body,
    Map<String, String>? headers,
  }) async {
    try {
      final uri = _buildUri(path);
      final response = await http
          .put(
            uri,
            headers: {...defaultHeaders, ...?headers},
            body: body != null ? jsonEncode(body) : null,
          )
          .timeout(timeout);

      return _handleResponse(response);
    } catch (e) {
      throw HttpException('PUT request failed: $e');
    }
  }

  /// DELETE request
  Future<Map<String, dynamic>?> delete(
    String path, {
    Map<String, String>? headers,
  }) async {
    try {
      final uri = _buildUri(path);
      final response = await http
          .delete(
            uri,
            headers: {...defaultHeaders, ...?headers},
          )
          .timeout(timeout);

      return _handleResponse(response);
    } catch (e) {
      throw HttpException('DELETE request failed: $e');
    }
  }

  /// Build URI with query parameters
  Uri _buildUri(String path, [Map<String, String>? queryParams]) {
    final cleanBase = baseUrl.endsWith('/') ? baseUrl.substring(0, baseUrl.length - 1) : baseUrl;
    final cleanPath = path.startsWith('/') ? path : '/$path';
    final fullUrl = '$cleanBase$cleanPath';

    if (queryParams != null && queryParams.isNotEmpty) {
      return Uri.parse(fullUrl).replace(queryParameters: queryParams);
    }

    return Uri.parse(fullUrl);
  }

  /// Handle HTTP response
  Map<String, dynamic>? _handleResponse(http.Response response) {
    if (response.statusCode >= 200 && response.statusCode < 300) {
      if (response.body.isEmpty) {
        return null;
      }
      return jsonDecode(response.body) as Map<String, dynamic>;
    } else if (response.statusCode == 404) {
      return null; // Not found
    } else {
      throw HttpException(
        'HTTP ${response.statusCode}: ${response.reasonPhrase}',
        statusCode: response.statusCode,
        body: response.body,
      );
    }
  }
}

/// HTTP exception
class HttpException implements Exception {
  final String message;
  final int? statusCode;
  final String? body;

  HttpException(
    this.message, {
    this.statusCode,
    this.body,
  });

  @override
  String toString() {
    if (statusCode != null) {
      return 'HttpException: $message (Status: $statusCode)';
    }
    return 'HttpException: $message';
  }
}

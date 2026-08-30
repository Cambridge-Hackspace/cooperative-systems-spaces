#!/bin/bash

# Test script for the authentication API with new user roles

API_BASE="http://localhost:4399/api"

echo "=== Testing Cooperative Systems Spaces Authentication API ==="
echo

# Test 1: Register a new user
echo "1. Registering a new user..."
REGISTER_RESPONSE=$(curl -s -X POST "${API_BASE}/auth/register" \
  -H "Content-Type: application/json" \
  -d '{
        "username": "testuser",
        "email": "test@example.com",
        "password": "testpassword123",
        "full_name": "Test User"
    }')

echo "Register response: $REGISTER_RESPONSE"
echo

# Test 2: Login with the new user
echo "2. Logging in with the new user..."
LOGIN_RESPONSE=$(curl -s -X POST "${API_BASE}/auth/login" \
  -H "Content-Type: application/json" \
  -d '{
        "username_or_email": "testuser",
        "password": "testpassword123"
    }')

echo "Login response: $LOGIN_RESPONSE"
echo

# Extract token from login response
TOKEN=$(echo "$LOGIN_RESPONSE" | grep -o '"token":"[^"]*"' | cut -d'"' -f4)
echo "Extracted token: $TOKEN"
echo

if [ -n "$TOKEN" ]; then
  # Test 3: Access protected route with token
  echo "3. Accessing user profile with valid token..."
  PROFILE_RESPONSE=$(curl -s -X GET "${API_BASE}/users/profile" \
    -H "Authorization: Bearer $TOKEN")

  echo "Profile response: $PROFILE_RESPONSE"
  echo

  # Test 4: Check user role in response
  echo "4. Checking user role in response..."
  USER_ROLE=$(echo "$LOGIN_RESPONSE" | grep -o '"role":"[^"]*"' | cut -d'"' -f4)
  echo "User role: $USER_ROLE"
  echo

  # Test 5: Try to access admin route (should fail)
  echo "5. Trying to access admin-only route (should fail)..."
  ADMIN_RESPONSE=$(curl -s -X GET "${API_BASE}/users/" \
    -H "Authorization: Bearer $TOKEN")

  echo "Admin route response: $ADMIN_RESPONSE"
  echo
else
  echo "No token received, skipping authenticated tests"
fi

# Test 6: Test invalid login
echo "6. Testing invalid login..."
INVALID_LOGIN=$(curl -s -X POST "${API_BASE}/auth/login" \
  -H "Content-Type: application/json" \
  -d '{
        "username_or_email": "nonexistent",
        "password": "wrongpassword"
    }')

echo "Invalid login response: $INVALID_LOGIN"
echo

# Test 7: Test access without token (should fail)
echo "7. Testing access without token (should fail)..."
NO_TOKEN_RESPONSE=$(curl -s -X GET "${API_BASE}/users/profile")

echo "No token response: $NO_TOKEN_RESPONSE"
echo

echo "=== Test completed ==="

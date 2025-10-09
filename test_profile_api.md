# Profile API Testing Guide

## API Endpoints

### 1. Get User Profile
```bash
GET /api/profiles/{user_id}
Authorization: Bearer <jwt_token>
```

### 2. Update User Profile
```bash
PUT /api/profiles/{user_id}
Authorization: Bearer <jwt_token>
Content-Type: application/json

{
    "profile": {
        "bio": "Software engineer with passion for cooperative systems",
        "phone": "+1-555-123-4567",
        "emergency_contact": "Jane Doe - spouse - +1-555-987-6543"
    }
}
```

### 3. Get Profile Configuration (Admin only)
```bash
GET /api/profiles/config
Authorization: Bearer <admin_jwt_token>
```

### 4. Update Profile Configuration (Admin only)
```bash
PUT /api/profiles/config
Authorization: Bearer <admin_jwt_token>
Content-Type: application/json

{
    "profiles_enabled": true,
    "profile_fields": [
        {
            "key": "bio",
            "label": "Bio",
            "field_type": "Text",
            "required": false,
            "help_text": "Tell us about yourself"
        },
        {
            "key": "phone",
            "label": "Phone Number",
            "field_type": "Phone",
            "required": false,
            "help_text": "Your contact phone number"
        },
        {
            "key": "skills",
            "label": "Skills",
            "field_type": {
                "Select": {
                    "options": ["Programming", "Electronics", "Woodworking", "3D Printing", "Design"]
                }
            },
            "required": false,
            "help_text": "What skills do you have?"
        }
    ]
}
```

## Testing Workflow

1. Start the server: `cargo run --bin css-server`
2. Register a user: `POST /api/auth/register`
3. Login to get JWT token: `POST /api/auth/login`
4. Test profile endpoints with the JWT token
5. Test admin endpoints (if user has admin role)

## Expected Responses

### Successful Profile Update
```json
{
    "success": true,
    "message": "Profile updated successfully",
    "data": {
        "user_id": "123e4567-e89b-12d3-a456-426614174000",
        "profile": {
            "bio": "Software engineer with passion for cooperative systems",
            "phone": "+1-555-123-4567",
            "emergency_contact": "Jane Doe - spouse - +1-555-987-6543"
        }
    }
}
```

### Profile Validation Error
```json
{
    "success": false,
    "error": "Profile validation failed: Field 'phone' must be a valid phone number"
}
```

### Permission Error
```json
{
    "success": false,
    "error": "You can only update your own profile"
}
```
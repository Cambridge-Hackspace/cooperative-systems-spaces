# Profile System Frontend Integration

## 🎯 Overview

This document outlines the complete frontend integration for the user profile system, including components, stores, and API integration.

## 📁 File Structure

```
frontend/src/
├── components/
│   ├── ProfileField.vue          # Dynamic form field component
│   ├── UserProfile.vue          # User profile display/edit component
│   └── ProfileConfigAdmin.vue    # Admin profile configuration
├── stores/
│   └── profile.ts               # Profile state management
├── views/
│   ├── ProfileView.vue          # Profile page view
│   └── AdminProfileConfigView.vue # Admin config page
├── utils/
│   └── api.ts                   # API client with profile endpoints
├── types/
│   └── index.ts                 # TypeScript type definitions
└── router/
    └── index.ts                 # Route definitions
```

## 🧩 Components

### ProfileField.vue
Dynamic form field component that renders different input types based on field configuration:

**Features:**
- ✅ Supports all profile field types (Text, Email, Phone, Number, Date, Boolean, Select)
- ✅ Real-time validation with error display
- ✅ Help text tooltips
- ✅ Required field indicators
- ✅ Disabled state for read-only mode

**Usage:**
```vue
<ProfileField
  :field="fieldConfig"
  v-model="fieldValue"
  :error-message="validationError"
  :disabled="!isEditing"
  @blur="validateField"
/>
```

### UserProfile.vue
Complete user profile management component:

**Features:**
- ✅ View/Edit mode toggle
- ✅ Permission-based editing (own profile + staff override)
- ✅ Real-time form validation
- ✅ Profile field rendering based on configuration
- ✅ Error handling and loading states
- ✅ Audit logging integration

**Usage:**
```vue
<UserProfile :user-id="userId" :user="userObject" />
```

### ProfileConfigAdmin.vue
Admin interface for configuring profile fields:

**Features:**
- ✅ CRUD operations for profile fields
- ✅ Live preview of profile form
- ✅ Field type selection with options
- ✅ Drag-and-drop field ordering (future enhancement)
- ✅ Validation for configuration integrity
- ✅ Select field option management

## 🗄️ State Management

### Profile Store (`stores/profile.ts`)

**State:**
- `profiles`: Cache of user profiles by user ID
- `profileConfig`: Current profile field configuration
- `loading`: Loading state for async operations
- `error`: Error state for failed operations

**Getters:**
- `getProfileForUser(userId)`: Get cached profile data
- `canEditProfile(userId)`: Check edit permissions
- `canManageProfileConfig`: Check admin permissions
- `isProfilesEnabled`: Check if profiles are enabled
- `getProfileFields`: Get current field configuration
- `getRequiredFields`: Get only required fields

**Actions:**
- `fetchUserProfile(userId)`: Load user profile from API
- `updateUserProfile(userId, data)`: Update user profile
- `fetchProfileConfig()`: Load profile configuration
- `updateProfileConfig(config)`: Update profile configuration
- `validateProfile(data)`: Validate profile data
- `clearError()`, `clearProfiles()`: Utility methods

## 🌐 API Integration

### Profile API (`utils/api.ts`)

**Endpoints:**
- `profileApi.getUserProfile(userId)`: GET `/api/profiles/{userId}`
- `profileApi.updateUserProfile(userId, data)`: PUT `/api/profiles/{userId}`
- `profileApi.getProfileConfig()`: GET `/api/profiles/config`
- `profileApi.updateProfileConfig(config)`: PUT `/api/profiles/config`

**Features:**
- ✅ Automatic JWT token injection
- ✅ Error handling and 401 redirect
- ✅ TypeScript typed responses
- ✅ Centralized API configuration

## 🛣️ Routing

### Route Configuration

**User Routes:**
- `/profile` → Redirects to `/profile/me`
- `/profile/me` → Current user's profile
- `/profile/:userId` → Specific user's profile
- `/users/:userId` → Alternative user profile route

**Admin Routes:**
- `/admin/profile-config` → Profile field configuration

**Guards:**
- Authentication required for all profile routes
- Admin role required for configuration routes
- Permission checks for viewing/editing other users' profiles

## 🎨 UI/UX Features

### Responsive Design
- ✅ Mobile-first approach with Tailwind CSS
- ✅ Card-based layout with DaisyUI components
- ✅ Responsive grid for form fields
- ✅ Touch-friendly form controls

### User Experience
- ✅ Loading states and progress indicators
- ✅ Error messages and validation feedback
- ✅ Success notifications
- ✅ Help text and field descriptions
- ✅ Intuitive edit/save workflow

### Accessibility
- ✅ Semantic HTML structure
- ✅ Proper ARIA labels and roles
- ✅ Keyboard navigation support
- ✅ Screen reader compatibility
- ✅ Color contrast compliance

## 🔧 Development Setup

### Prerequisites
```bash
# Install dependencies
cd frontend
npm install

# Start development server
npm run dev
```

### Environment Variables
```env
# Add to .env.local
VITE_API_BASE_URL=http://localhost:4399
```

### Build and Deploy
```bash
# Build for production
npm run build

# Preview build
npm run preview
```

## 🧪 Testing

### Component Testing
```bash
# Run component tests
npm run test:unit

# Test specific component
npm run test:unit ProfileField.vue
```

### Integration Testing
```bash
# Run integration tests
npm run test:integration

# Test API integration
npm run test:api
```

## 🚀 Usage Examples

### Basic Profile Display
```vue
<template>
  <div class="container mx-auto">
    <UserProfile :user-id="currentUserId" />
  </div>
</template>

<script setup>
import { useAuthStore } from '@/stores/auth'
const authStore = useAuthStore()
const currentUserId = computed(() => authStore.user?.id)
</script>
```

### Admin Configuration
```vue
<template>
  <div class="admin-panel">
    <h1>Profile Settings</h1>
    <ProfileConfigAdmin />
  </div>
</template>
```

### Custom Field Validation
```ts
// Add custom validation in profile store
function validateCustomField(fieldKey: string, value: any): string | null {
  if (fieldKey === 'social_security' && value) {
    const ssnRegex = /^\d{3}-\d{2}-\d{4}$/
    if (!ssnRegex.test(value)) {
      return 'SSN must be in format XXX-XX-XXXX'
    }
  }
  return null
}
```

## 🎯 Future Enhancements

### Planned Features
- [ ] **Profile Templates**: Predefined field sets for different user types
- [ ] **Bulk Field Management**: Import/export field configurations
- [ ] **Field Dependencies**: Conditional field visibility
- [ ] **File Uploads**: Profile picture and document attachments
- [ ] **Profile Visibility**: Public/private profile settings
- [ ] **Profile History**: Track changes over time
- [ ] **Advanced Validation**: Custom validation rules
- [ ] **Multi-language Support**: Internationalized field labels

### Performance Optimizations
- [ ] **Lazy Loading**: Load profiles on-demand
- [ ] **Caching Strategy**: Smart cache invalidation
- [ ] **Virtual Scrolling**: For large user lists
- [ ] **Image Optimization**: Profile picture compression

### Accessibility Improvements
- [ ] **Voice Control**: Voice-to-text input
- [ ] **High Contrast Mode**: Enhanced visual accessibility
- [ ] **Screen Reader**: Enhanced announcements
- [ ] **Keyboard Shortcuts**: Power user shortcuts

## 📚 Additional Resources

- **DaisyUI Documentation**: https://daisyui.com/
- **Vue 3 Composition API**: https://vuejs.org/guide/composition-api/
- **Pinia State Management**: https://pinia.vuejs.org/
- **TypeScript Guide**: https://www.typescriptlang.org/docs/

## 🤝 Contributing

1. Follow the existing code style and patterns
2. Add TypeScript types for all new interfaces
3. Include comprehensive error handling
4. Write unit tests for new components
5. Update this documentation for new features

---

The profile system frontend is now fully integrated and ready for production use! 🎉
<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal" @click.stop>
      <div class="modal-header">
        <h3>Assign Trainer to {{ tool.name }}</h3>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <div v-if="loading" class="loading">
          <div class="spinner"></div>
          <p>Loading users...</p>
        </div>

        <div v-else-if="availableUsers.length === 0" class="no-users">
          <p>No available users to assign as trainers.</p>
        </div>

        <form @submit.prevent="submitForm">
          <div class="form-group">
            <label for="user">Select User</label>
            <select id="user" v-model="formData.user_id" class="form-control" required>
              <option value="">Choose a user...</option>
              <option v-for="user in availableUsers" :key="user.id" :value="user.id">
                {{ user.full_name || user.username }} ({{ user.email }})
              </option>
            </select>
          </div>

          <div class="form-group">
            <label for="expires_at">Expiration Date (Optional)</label>
            <input
              id="expires_at"
              v-model="formData.expires_at"
              type="date"
              class="form-control"
              :min="today"
            />
            <small class="form-text">Leave blank for no expiration</small>
          </div>

          <div class="form-group">
            <label for="notes">Notes (Optional)</label>
            <textarea
              id="notes"
              v-model="formData.notes"
              class="form-control"
              rows="3"
              placeholder="Add any notes about this trainer assignment..."
            ></textarea>
          </div>

          <div v-if="error" class="error">{{ error }}</div>

          <div class="modal-actions">
            <button type="button" class="btn btn-secondary" @click="$emit('close')">Cancel</button>
            <button type="submit" :disabled="submitting" class="btn btn-primary">
              {{ submitting ? 'Assigning...' : 'Assign Trainer' }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { trainerApi, userApi } from '../utils/api'
import type { Tool, AssignTrainerRequest, User } from '../types'

interface Props {
  tool: Tool
  existingTrainers: string[]
}

const props = defineProps<Props>()
defineEmits<{
  close: []
  assigned: []
}>()

// State
const users = ref<User[]>([])
const loading = ref(false)
const submitting = ref(false)
const error = ref('')

const formData = ref({
  user_id: '',
  notes: '',
  expires_at: '',
})

// Computed
const today = computed(() => {
  return new Date().toISOString().split('T')[0]
})

const availableUsers = computed(() => {
  return users.value.filter((user) => user.is_active && !props.existingTrainers.includes(user.id))
})

// Methods
const loadUsers = async () => {
  try {
    loading.value = true
    error.value = ''

    const response = await userApi.getAllUsers()

    if (response.success && response.data?.items) {
      users.value = response.data.items
      console.log('Loaded users:', users.value.length)
    } else {
      error.value = response.error || 'Failed to load users'
      console.error('Failed to load users:', response)
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load users'
    console.error('Error loading users:', err)
  } finally {
    loading.value = false
  }
}

const submitForm = async () => {
  try {
    submitting.value = true
    error.value = ''

    // Prepare the data
    const requestData: AssignTrainerRequest = {
      user_id: formData.value.user_id,
      tool_id: props.tool.id,
      notes: formData.value.notes || undefined,
      expires_at: formData.value.expires_at || undefined,
    }

    const response = await trainerApi.assignToolTrainer(requestData)

    if (response.success) {
      console.log('Loaded users:', users.value.length)
    } else {
      error.value = response.error || 'Failed to load users'
      console.error('Failed to load users:', response)
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load users'
    console.error('Error loading users:', err)
  } finally {
    submitting.value = false
  }
}

// Lifecycle
onMounted(() => {
  void loadUsers()
})
</script>

<style scoped>
.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid #f3f3f3;
  border-top: 3px solid #3498db;
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto;
}

@keyframes spin {
  0% {
    transform: rotate(0deg);
  }
  100% {
    transform: rotate(360deg);
  }
}

.loading {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
}

.loading p {
  margin-top: 1rem;
  margin-bottom: 0;
}

.no-users {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
  background: #f8f9fa;
  border-radius: 4px;
  margin-bottom: 1rem;
}

.no-users p {
  margin: 0;
}

<style scoped > .modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1100;
}

.modal {
  background: white;
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  width: 90%;
  max-width: 500px;
  max-height: 90vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.modal-header {
  padding: 1.5rem;
  border-bottom: 1px solid #e1e5e9;
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: #f8f9fa;
}

.modal-header h3 {
  margin: 0;
  color: #2c3e50;
  font-size: 1.25rem;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: #6c757d;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  transition: all 0.2s;
}

.close-btn:hover {
  background: #e9ecef;
  color: #495057;
}

.modal-body {
  padding: 1.5rem;
  overflow-y: auto;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group:last-child {
  margin-bottom: 0;
}

label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  color: #2c3e50;
}

.form-control {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 1rem;
  transition: border-color 0.2s;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.form-text {
  font-size: 0.875rem;
  color: #6c757d;
  margin-top: 0.25rem;
}

.error {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  border: 1px solid #f5c6cb;
  margin-bottom: 1rem;
}

.modal-actions {
  display: flex;
  gap: 0.75rem;
  justify-content: flex-end;
  margin-top: 1.5rem;
  padding-top: 1rem;
  border-top: 1px solid #e1e5e9;
}

.btn {
  padding: 0.75rem 1.5rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 1rem;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: #007bff;
  color: white;
  border-color: #007bff;
}

.btn-primary:hover:not(:disabled) {
  background: #0056b3;
  border-color: #0056b3;
}

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover {
  background: #545b62;
  border-color: #545b62;
}

select.form-control {
  cursor: pointer;
}

textarea.form-control {
  resize: vertical;
  min-height: 80px;
}

@media (max-width: 768px) {
  .modal {
    width: 95%;
    margin: 1rem;
  }

  .modal-header,
  .modal-body {
    padding: 1rem;
  }

  .modal-actions {
    flex-direction: column;
  }

  .btn {
    width: 100%;
  }
}
</style>

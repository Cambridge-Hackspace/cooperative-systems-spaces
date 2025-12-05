<template>
  <div class="modal-overlay" @click="closeModal">
    <div class="modal-content trainer-modal bg-base-100" @click.stop>
      <div class="modal-header bg-gradient-to-br from-primary via-secondary to-primary">
        <div>
          <h3 class="font-bold">Manage Trainers - {{ tool.name }}</h3>
          <p class="subtitle font-bold">Assign and manage authorized trainers for this tool</p>
        </div>
        <button @click="closeModal" class="close-btn">&times;</button>
      </div>

      <div class="modal-body">
        <div class="trainer-management-content">
  <div class="trainer-management bg-secondary text-secondary-content">
    <div class="trainer-header">
      <button
        v-if="canManageTrainers"
        @click="showAssignForm = !showAssignForm"
        class="btn btn-primary"
      >
        <i class="icon-plus"></i>
        {{ showAssignForm ? 'Cancel' : 'Assign Trainer' }}
      </button>
    </div>

    <!-- Inline Assign Trainer Form -->
    <div v-if="showAssignForm" class="assign-trainer-form bg-primary text-primary-content">
      <h4>Assign New Trainer</h4>
      
      <div v-if="loadingUsers" class="loading">
        <div class="spinner"></div>
        <p>Loading users...</p>
      </div>
      
      <div v-else-if="availableUsers.length === 0" class="no-users">
        <p>No available users to assign as trainers.</p>
      </div>
      
      <form v-else @submit.prevent="submitAssignForm" class="assign-form">
        <div class="form-group">
          <label for="user">Select User</label>
          <select 
            id="user"
            v-model="assignFormData.user_id"
            class="form-control select"
            required
          >
            <option value="">Choose a user...</option>
            <option 
              v-for="user in availableUsers" 
              :key="user.id"
              :value="user.id"
            >
              {{ user.full_name || user.username }} ({{ user.email }})
            </option>
          </select>
        </div>

        <div class="form-group">
          <label for="expires_at">Expiration Date (Optional)</label>
          <input 
            id="expires_at"
            v-model="assignFormData.expires_at"
            type="date"
            class="form-control input"
            :min="today"
          />
          <small class="form-text">Leave blank for no expiration</small>
        </div>

        <div class="form-group">
          <label for="notes">Notes (Optional)</label>
          <textarea 
            id="notes"
            v-model="assignFormData.notes"
            class="form-control textarea"
            rows="3"
            placeholder="Add any notes about this trainer assignment..."
          ></textarea>
        </div>

        <div v-if="assignError" class="error">{{ assignError }}</div>

        <div class="form-actions">
          <button type="submit" :disabled="assignSubmitting" class="btn btn-success">
            {{ assignSubmitting ? 'Assigning...' : 'Assign Trainer' }}
          </button>
        </div>
      </form>
    </div>

    <div class="trainer-list" v-if="trainers.length > 0">
      <div 
        v-for="trainerWithUser in trainers" 
        :key="trainerWithUser.trainer.id"
        class="trainer-item"
        :class="{ 'inactive': !trainerWithUser.trainer.is_active }"
      >
        <div class="trainer-info">
          <div class="trainer-name">
            <h4>{{ trainerWithUser.user_full_name || trainerWithUser.user_name }}</h4>
            <span class="trainer-email">{{ trainerWithUser.user_email }}</span>
          </div>
          
          <div class="trainer-meta">
            <div class="status">
              <span 
                :class="{
                  'status-active': trainerWithUser.trainer.is_active && !isExpired(trainerWithUser.trainer),
                  'status-inactive': !trainerWithUser.trainer.is_active,
                  'status-expired': isExpired(trainerWithUser.trainer)
                }"
              >
                {{ getTrainerStatus(trainerWithUser.trainer) }}
              </span>
            </div>

            <div class="dates">
              <div class="authorized-date">
                Authorized: {{ formatDate(trainerWithUser.trainer.authorized_at) }}
              </div>
              <div v-if="trainerWithUser.trainer.expires_at" class="expires-date">
                Expires: {{ formatDate(trainerWithUser.trainer.expires_at) }}
              </div>
            </div>

            <div v-if="trainerWithUser.trainer.notes" class="notes">
              {{ trainerWithUser.trainer.notes }}
            </div>
          </div>
        </div>

        <div class="trainer-actions" v-if="canManageTrainers">
          <button 
            @click="editTrainer(trainerWithUser)"
            class="btn btn-sm btn-secondary"
          >
            Edit
          </button>
          <button 
            v-if="trainerWithUser.trainer.is_active"
            @click="deactivateTrainer(trainerWithUser.trainer)"
            class="btn btn-sm btn-warning"
          >
            Deactivate
          </button>
          <button 
            v-else
            @click="activateTrainer(trainerWithUser.trainer)"
            class="btn btn-sm btn-success"
          >
            Activate
          </button>
          <button 
            @click="removeTrainer(trainerWithUser.trainer)"
            class="btn btn-sm btn-danger"
          >
            Remove
          </button>
        </div>
      </div>
    </div>

    <div v-else-if="!loading" class="no-trainers">
      <p>No trainers assigned to this tool.</p>
      <button 
        v-if="canManageTrainers"
        @click="showAssignForm = true"
        class="btn btn-primary"
      >
        Assign First Trainer
      </button>
    </div>

    <div v-if="loading" class="loading">Loading trainers...</div>
    <div v-if="error" class="error">{{ error }}</div>

    <!-- Edit Trainer Modal -->
    <EditTrainerModal
      v-if="showEditModal && selectedTrainer"
      :tool="tool"
      :trainer-with-user="selectedTrainer"
      @close="showEditModal = false"
      @updated="onTrainerUpdated"
    />
  </div>
        </div>
      </div>
      
      <div class="modal-footer bg-base-200">
        <button @click="closeModal" class="btn btn-secondary">Close</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '../stores/auth'
import { trainerApi, userApi } from '../utils/api'
import type { Tool, ToolTrainerWithUser, ToolTrainer } from '../types'
import EditTrainerModal from './EditTrainerModal.vue'

interface Props {
  tool: Tool
}

const props = defineProps<Props>()
const emit = defineEmits<{
  'trainer-updated': []
  'close': []
}>()

const closeModal = () => {
  emit('close')

}
const auth = useAuthStore()

// State
const trainers = ref<ToolTrainerWithUser[]>([])
const users = ref<User[]>([])
const loading = ref(false)
const loadingUsers = ref(false)
const error = ref('')
const assignError = ref('')
const assignSubmitting = ref(false)
const showAssignForm = ref(false)
const assignFormData = ref({
  user_id: '',
  notes: '',
  expires_at: ''
})
const showEditModal = ref(false)
const selectedTrainer = ref<ToolTrainerWithUser | null>(null)
const includeInactive = ref(false)

// Computed
const canManageTrainers = computed(() => {
  const userRole = auth.user?.role?.toLowerCase()
  return userRole === 'staff' || userRole === 'admin'
})

const today = computed(() => {
  return new Date().toISOString().split('T')[0]
})

const availableUsers = computed(() => {
  return users.value.filter(user => 
    user.is_active && 
    !trainers.value.map(t => t.trainer.user_id).includes(user.id)
  )
})

// Import User type
import type { User } from '../types'

// Methods
const loadUsers = async () => {
  try {
    loadingUsers.value = true
    assignError.value = ''
    
    const response = await userApi.getAllUsers()
    
    if (response.success && response.data?.items) {
      users.value = response.data.items
    } else {
      assignError.value = response.error || 'Failed to load users'
    }
  } catch (err: any) {
    assignError.value = err.message || 'Failed to load users'
  } finally {
    loadingUsers.value = false
  }
}

const loadTrainers = async () => {
  try {
    loading.value = true
    error.value = ''
    
    const response = await trainerApi.getToolTrainers(props.tool.id, includeInactive.value)
    
    if (response.success && response.data) {
      trainers.value = response.data
    } else {
      error.value = response.error || 'Failed to load trainers'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load trainers'
  } finally {
    loading.value = false
  }
}

const isExpired = (trainer: ToolTrainer): boolean => {
  if (!trainer.expires_at) return false
  return new Date(trainer.expires_at) < new Date()
}

const getTrainerStatus = (trainer: ToolTrainer): string => {
  if (!trainer.is_active) return 'Inactive'
  if (isExpired(trainer)) return 'Expired'
  return 'Active'
}

const formatDate = (dateString: string): string => {
  return new Date(dateString).toLocaleDateString()
}

const editTrainer = (trainerWithUser: ToolTrainerWithUser) => {
  selectedTrainer.value = trainerWithUser
  showEditModal.value = true
}

const deactivateTrainer = async (trainer: ToolTrainer) => {
  if (!confirm('Are you sure you want to deactivate this trainer?')) return

  try {
    const response = await trainerApi.updateToolTrainer(
      props.tool.id,
      trainer.user_id,
      { is_active: false }
    )

    if (response.success) {
      await loadTrainers()
      emit('trainer-updated')
    } else {
      error.value = response.error || 'Failed to deactivate trainer'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to deactivate trainer'
  }
}

const activateTrainer = async (trainer: ToolTrainer) => {
  try {
    const response = await trainerApi.updateToolTrainer(
      props.tool.id,
      trainer.user_id,
      { is_active: true }
    )

    if (response.success) {
      await loadTrainers()
      emit('trainer-updated')
    } else {
      error.value = response.error || 'Failed to activate trainer'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to activate trainer'
  }
}

const removeTrainer = async (trainer: ToolTrainer) => {
  if (!confirm('Are you sure you want to permanently remove this trainer? This action cannot be undone.')) return

  try {
    const response = await trainerApi.removeToolTrainer(props.tool.id, trainer.user_id)

    if (response.success) {
      await loadTrainers()
      emit('trainer-updated')
    } else {
      error.value = response.error || 'Failed to remove trainer'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to remove trainer'
  }
}

const submitAssignForm = async () => {
  try {
    assignSubmitting.value = true
    assignError.value = ''

    // Prepare the data
    const requestData = {
      user_id: assignFormData.value.user_id,
      tool_id: props.tool.id,
      notes: assignFormData.value.notes || undefined,
      expires_at: assignFormData.value.expires_at || undefined
    }

    const response = await trainerApi.assignToolTrainer(requestData)

    if (response.success) {
      // Reset form
      assignFormData.value = {
        user_id: '',
        notes: '',
        expires_at: ''
      }
      showAssignForm.value = false
      await loadTrainers()
      emit('trainer-updated')
    } else {
      assignError.value = response.error || 'Failed to assign trainer'
    }
  } catch (err: any) {
    assignError.value = err.message || 'Failed to assign trainer'
  } finally {
    assignSubmitting.value = false
  }
}

const onTrainerAssigned = () => {
  loadTrainers()
  emit('trainer-updated')
}

const onTrainerUpdated = () => {
  showEditModal.value = false
  selectedTrainer.value = null
  loadTrainers()
  emit('trainer-updated')
}

const toggleInactive = () => {
  includeInactive.value = !includeInactive.value
  loadTrainers()
}

// Lifecycle
onMounted(() => {
  loadTrainers()
  // Load users when form is first shown
  if (canManageTrainers.value) {
    loadUsers()
  }
})
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal-content {
  border-radius: 12px;
  max-width: 800px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.15);
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 2rem;
  border-bottom: 1px solid #e1e5e9;
  border-radius: 12px 12px 0 0;
}

.modal-header h3 {
  margin: 0 0 0.25rem 0;
  font-size: 1.5rem;
}

.subtitle {
  margin: 0;
  opacity: 0.9;
  font-size: 0.9rem;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: white;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  opacity: 0.8;
  transition: opacity 0.2s;
}

.close-btn:hover {
  opacity: 1;
}

.modal-body {
  padding: 2rem;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  padding: 1.5rem 2rem;
  border-top: 1px solid #e1e5e9;
  border-radius: 0 0 12px 12px;
}

.trainer-management-content {
  max-height: none;
}

.assign-trainer-form {
  border: 1px solid #e9ecef;
  border-radius: 8px;
  padding: 1.5rem;
  margin: 1rem 1.5rem;
}

.assign-trainer-form h4 {
  margin: 0 0 1.5rem 0;
  font-size: 1.1rem;
}

.assign-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.form-group {
  display: flex;
  flex-direction: column;
}

.form-group label {
  font-weight: 500;
  margin-bottom: 0.5rem;
}

.form-control {
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
  margin-top: 0.25rem;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 0.5rem;
}

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
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
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
  background: #fff;
  border-radius: 4px;
  margin-bottom: 1rem;
}

.no-users p {
  margin: 0;
}

select.form-control {
  cursor: pointer;
}

textarea.form-control {
  resize: vertical;
  min-height: 80px;
}

.trainer-management {
  border: 1px solid #e1e5e9;
  border-radius: 8px;
  overflow: hidden;
}

.trainer-header {
  padding: 1rem 1.5rem;
  border-bottom: 1px solid #e1e5e9;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.trainer-header h3 {
  margin: 0;
  color: #2c3e50;
}

.trainer-list {
  padding: 1rem 0;
}

.trainer-item {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 1rem 1.5rem;
  border-bottom: 1px solid #f1f3f4;
  transition: background-color 0.2s;
}

.trainer-item:last-child {
  border-bottom: none;
}

.trainer-item:hover {
  background-color: #f8f9fa;
}

.trainer-item.inactive {
  opacity: 0.6;
  background-color: #f8f8f8;
}

.trainer-info {
  flex: 1;
}

.trainer-name h4 {
  margin: 0 0 0.25rem 0;
  color: #2c3e50;
  font-size: 1.1rem;
}

.trainer-email {
  color: #6c757d;
  font-size: 0.9rem;
}

.trainer-meta {
  margin-top: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.status span {
  padding: 0.25rem 0.75rem;
  border-radius: 12px;
  font-size: 0.8rem;
  font-weight: 500;
  text-transform: uppercase;
}

.status-active {
  background: #d4edda;
  color: #155724;
}

.status-inactive {
  background: #f8d7da;
  color: #721c24;
}

.status-expired {
  background: #fff3cd;
  color: #856404;
}

.dates {
  display: flex;
  gap: 1rem;
  font-size: 0.85rem;
  color: #6c757d;
}

.notes {
  font-size: 0.9rem;
  color: #495057;
  font-style: italic;
}

.trainer-actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  flex-shrink: 0;
}

.no-trainers {
  text-align: center;
  padding: 2rem;
}

.no-trainers p {
  margin-bottom: 1rem;
}

.loading {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
}

.error {
  background: #f8d7da;
  color: #721c24;
  padding: 1rem;
  margin: 1rem;
  border-radius: 4px;
  border: 1px solid #f5c6cb;
}

.btn {
  padding: 0.375rem 0.75rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.875rem;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}


.icon-plus::before {
  content: '+';
  font-weight: bold;
}

@media (max-width: 768px) {
  .trainer-item {
    flex-direction: column;
    align-items: stretch;
    gap: 1rem;
  }
  
  .trainer-actions {
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  
  .dates {
    flex-direction: column;
    gap: 0.25rem;
  }
}
</style>
